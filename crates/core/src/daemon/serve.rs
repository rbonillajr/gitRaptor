//! The daemon's channel: serving it, the wiring of its protected operations
//! and taking it back when its socket is replaced (ADR-GRP-005 § 5).

#[cfg(any(unix, windows))]
use std::sync::Arc;

#[cfg(any(unix, windows))]
use gitraptor_api::event::ENGINE_STATE;
#[cfg(any(unix, windows))]
use gitraptor_api::messages::DaemonView;

use super::Daemon;
#[cfg(any(unix, windows))]
use super::profile_error_kind;

#[cfg(any(unix, windows))]
use crate::timemachine::protected::{DaemonBackend, TimeMachineBackend};

impl Daemon {
    /// The protected-operation wiring: the configured double, or the
    /// daemon's own repo layer when a catalog of operations is wired.
    #[cfg(any(unix, windows))]
    pub(super) fn protected_wiring(&self) -> Option<crate::channel::ProtectedWiring> {
        if let Some(wiring) = &self.config.protected {
            return Some(wiring.clone());
        }
        let ops = self.config.operations.clone()?;
        let mut wiring = crate::channel::ProtectedWiring::new(
            Arc::new(DaemonBackend::new(
                Arc::clone(&self.tm),
                ops.clone(),
                Arc::clone(&self.mcp_repos),
            )),
            Arc::clone(&ops.gate),
            ops.prior_deadline,
        );
        // Honored only in debug builds (tests), like `prior_layer`.
        wiring.test_layer_override = ops.test_layer_override.filter(|_| cfg!(debug_assertions));
        Some(wiring)
    }

    /// The Time Machine's own commands over the daemon's repo layer
    /// (US-TMC-002), wired whether or not a catalog of operations is.
    #[cfg(any(unix, windows))]
    pub(super) fn time_machine_wiring(
        &self,
        challenges: Arc<crate::timemachine::protected::ChallengeBook>,
    ) -> crate::channel::TimeMachineWiring {
        let layer = self
            .config
            .tm_prior_layer
            .clone()
            .filter(|_| cfg!(debug_assertions))
            .map(|l| l.0);
        crate::channel::TimeMachineWiring {
            backend: Arc::new(TimeMachineBackend::new(
                Arc::clone(&self.tm),
                layer,
                Arc::clone(&self.mcp_repos),
            )),
            git: self.report.git.clone(),
            invoker: self.config.env.invoker(),
            prior_deadline: crate::timemachine::protected::DEFAULT_PRIOR_DEADLINE,
            challenges,
        }
    }

    #[cfg(any(unix, windows))]
    pub(super) fn serve_channel(&mut self) {
        let Some(bound) = self.bound.take() else {
            return;
        };
        let protected = self.protected_wiring();
        // The executor's book when one is wired, so "one live challenge per connection" holds
        // for the whole daemon.
        let challenges = protected
            .as_ref()
            .map_or_else(Default::default, |p| p.executor.challenges());
        let args = crate::channel::ServeArgs {
            config: self.config.channel.clone(),
            bus: Arc::clone(&self.bus),
            control: self.handle.clone(),
            logger: self.logger.clone(),
            instance_id: self.profile.instance_id().to_owned(),
            daemon: DaemonView {
                pid: std::process::id(),
                protocol: self.config.channel.protocol,
                binary_version: crate::version().to_owned(),
                started_wall_ms: self.started_ms,
            },
            protected,
            time_machine: Some(self.time_machine_wiring(challenges)),
            tm_engine: Some(self.capture_deps()),
            resources: Arc::clone(&self.resources),
            guard: Arc::clone(&self.guard),
            mcp_repos: Arc::clone(&self.mcp_repos),
            hook_claims: Arc::clone(&self.hook_claims),
        };
        match crate::channel::Server::serve(bound, args) {
            Ok(server) => {
                self.server = Some(server);
                let view = self.bus.snapshot().1.engine;
                self.bus.publish(ENGINE_STATE, view, None, |_| {});
                self.logger.info("channel_serving", &[]);
            }
            Err(_) => self.logger.error("channel_serve_failed", &[]),
        }
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn serve_channel(&mut self) {}

    /// If another process replaced the socket file, take the channel back:
    /// close every connection, bind again and serve.
    #[cfg(any(unix, windows))]
    pub(super) fn check_channel(&mut self) {
        let Some(server) = self.server.as_mut() else {
            return;
        };
        if server.socket_intact() {
            return;
        }
        self.logger.error("channel_socket_replaced", &[]);
        let runtime = server.runtime().to_path_buf();
        server.shutdown();
        self.server = None;
        match crate::channel::BoundChannel::bind(&runtime) {
            Ok(bound) => {
                self.bound = Some(bound);
                self.serve_channel();
            }
            Err(err) => self.logger.error(
                "channel_bind_failed",
                &[("kind", profile_error_kind(&err).into())],
            ),
        }
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn check_channel(&mut self) {}
}
