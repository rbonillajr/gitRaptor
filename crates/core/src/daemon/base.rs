//! The repo's base branch at runtime: when Guardrails confirms one, the snapshot takes it and
//! the modules hear of it, so what reads the published base (the predictor) plans against it.

use gitraptor_api::{Timings, clock};

use super::{Daemon, repo_base};
use crate::observe;

impl Daemon {
    /// Publishes the repo's base from its store when it differs from the published one: the
    /// worktree views with the new base, then their ahead/behind counted against it, then
    /// the modules' `base_changed`. A repo with nothing published yet (no
    /// first read) or whose base did not change publishes nothing and notifies no module.
    pub(super) fn refresh_base(&self, repo_id: &str) {
        let base = repo_base(&self.stores, repo_id);
        let shared = self.bus.snapshot().1;
        let (Some(inputs), Some(repo)) = (
            shared.divergence.get(repo_id),
            shared.repos.iter().find(|r| r.repo_id == repo_id),
        ) else {
            return;
        };
        if inputs.base == base {
            return;
        }
        // Not a change observed in a batch: every instant is now.
        let now = clock::monotonic_ns();
        let timings = Timings {
            batch_id: self.bus.next_batch(),
            t_recv: now,
            t_flush: now,
            t_computed: now,
            t_persisted: now,
            t_published: 0,
        };
        let view = observe::base_view(&base);
        let inputs = observe::DivergenceInputs {
            base,
            ..inputs.clone()
        };
        self.publish_views(
            repo_id,
            repo.worktrees.clone(),
            inputs,
            Some(view),
            Vec::new(),
            timings,
        );
        self.publish_divergence(repo_id, timings);
        self.modules.base_changed(repo_id);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gitraptor_testkit::Fixture;

    use super::*;
    use crate::daemon::{DaemonConfig, DaemonEnv, LogLimits};
    use crate::profile::ProfileDirs;

    /// A real daemon over a temporary repo and profile (NFR-01), started and not run.
    fn started() -> (Daemon, String, Fixture, tempfile::TempDir) {
        let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
        let worktree = gitraptor_testkit::fixture::canonical_dir(&fx.repo);
        let tmp = tempfile::tempdir().expect("a temporary dir");
        let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
        let (mut profile, _) = crate::profile::Profile::open(dirs.clone()).expect("a profile");
        let (entry, _) = profile
            .add_repo(
                &gitraptor_testkit::fixture::canonical_dir(&worktree.join(".git")),
                None,
                1,
            )
            .expect("the repo is added");
        drop(profile);
        let env = DaemonEnv::from_vars(std::env::vars_os());
        let daemon = Daemon::start(DaemonConfig {
            dirs,
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel: Default::default(),
            protected: None,
            operations: None,
            tm_prior_layer: None,
            tiers: Default::default(),
            discovery: Default::default(),
            power: Default::default(),
            predict: Default::default(),
            tm_capture: Default::default(),
        })
        .expect("the daemon starts");
        (daemon, entry.repo_id, fx, tmp)
    }

    #[test]
    fn same_base_publishes_nothing() {
        let (daemon, repo_id, _fx, _tmp) = started();
        let (seq, shared) = daemon.bus.snapshot();
        assert!(
            shared.divergence.contains_key(&repo_id),
            "the repo's first read is published"
        );
        let batch = daemon.bus.next_batch();

        daemon.refresh_base(&repo_id);
        daemon.refresh_base("not-observed");

        assert_eq!(daemon.bus.snapshot().0, seq, "nothing is published");
        // No batch was opened: neither the views nor the recount nor the modules were reached.
        assert_eq!(daemon.bus.next_batch(), batch + 1);
    }
}
