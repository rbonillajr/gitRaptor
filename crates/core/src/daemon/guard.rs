//! The Guardrails requests the daemon serves and the recovery of their
//! installs at start (US-GRD-001).

use gitraptor_git::SystemGit;

use super::{Daemon, DaemonConfig, Field, Logger, now_ms, raptor_path};
use super::{GuardReply, GuardRequest};

use crate::guardrails::install::{GuardCtx, InstallError};
use crate::guardrails::{GuardRegistry, install as guard_install};
use crate::profile::{Profile, RepoState, RepoStore};

impl Daemon {
    /// A Guardrails request for an observed repo (US-GRD-001). The loop is the only writer
    /// of the repo stores; the install itself writes the repo through the Guardrails write
    /// layer (ADR-GRD-001 § 7).
    pub(super) fn guard(
        &mut self,
        common_dir: &std::path::Path,
        request: GuardRequest,
    ) -> GuardReply {
        let entry = match self.profile.repo_by_common_dir(common_dir) {
            Ok(Some(entry)) if entry.state == RepoState::Observed => entry,
            Ok(_) => return GuardReply::NotObserved,
            Err(_) => return GuardReply::Failed,
        };
        let (Some(git), Some(raptor)) = (self.report.git.clone(), self.raptor_path()) else {
            return GuardReply::Failed;
        };
        let invoker = self.config.env.invoker();
        let instance = self.profile.instance_id().to_owned();
        let Some((_, store)) = self.stores.iter_mut().find(|(id, _)| *id == entry.repo_id) else {
            return GuardReply::Failed;
        };
        let ctx = GuardCtx {
            git: &git,
            invoker: &invoker,
            dirs: &self.config.dirs,
            instance: &instance,
            raptor: &raptor,
        };
        let common = entry.canonical_path.as_path();
        match request {
            GuardRequest::Plan => GuardReply::Plan(Box::new(guard_install::plan(
                &ctx,
                &entry.repo_id,
                common,
                store,
            ))),
            GuardRequest::Status => GuardReply::Status(Box::new(guard_install::status(
                &entry.repo_id,
                common,
                store,
            ))),
            GuardRequest::Decline => {
                self.logger
                    .info("guard_declined", &[("repo", Field::id(&entry.repo_id))]);
                GuardReply::Status(Box::new(guard_install::decline(
                    &entry.repo_id,
                    common,
                    store,
                )))
            }
            GuardRequest::Install => {
                match guard_install::install(
                    &ctx,
                    &entry.repo_id,
                    common,
                    store,
                    &self.guard,
                    now_ms(),
                ) {
                    Ok(status) => {
                        self.logger
                            .info("guard_installed", &[("repo", Field::id(&entry.repo_id))]);
                        GuardReply::Status(Box::new(status))
                    }
                    Err(InstallError::Rejected(blockers)) => {
                        self.logger.info(
                            "guard_install_refused",
                            &[
                                ("repo", Field::id(&entry.repo_id)),
                                ("blockers", blockers.len().into()),
                            ],
                        );
                        GuardReply::Rejected(blockers)
                    }
                    Err(InstallError::Failed(_)) => {
                        self.logger.error(
                            "guard_install_failed",
                            &[("repo", Field::id(&entry.repo_id))],
                        );
                        GuardReply::Failed
                    }
                }
            }
        }
    }
}

/// Startup recovery of the Guardrails installs of every observed repo.
pub(super) fn recover_guardrails(
    config: &DaemonConfig,
    profile: &Profile,
    logger: &Logger,
    git: Option<&SystemGit>,
    stores: &mut [(String, RepoStore)],
    registry: &GuardRegistry,
) {
    let Some(raptor) = raptor_path(config) else {
        return;
    };
    let invoker = config.env.invoker();
    for (repo_id, store) in stores.iter_mut() {
        let Ok(Some(entry)) = profile.repo(repo_id) else {
            continue;
        };
        let Some(git) = git else {
            // Without Git only the confirmed installs are published; an unfinished one waits.
            if let Some(journal) = store
                .guard_keys()
                .ok()
                .and_then(|k| k.journal)
                .and_then(|j| crate::guardrails::journal::Journal::from_json(&j))
                .filter(|j| j.stage == crate::guardrails::journal::Stage::Confirmed)
            {
                guard_install::publish(&config.dirs, repo_id, &journal, registry);
            }
            continue;
        };
        let ctx = GuardCtx {
            git,
            invoker: &invoker,
            dirs: &config.dirs,
            instance: profile.instance_id(),
            raptor: &raptor,
        };
        match guard_install::recover(&ctx, repo_id, &entry.canonical_path, store, registry) {
            guard_install::Recovery::Nothing => {}
            guard_install::Recovery::Confirmed => {
                logger.info("guard_install_recovered", &[("repo", Field::id(repo_id))]);
            }
            guard_install::Recovery::RolledBack => {
                logger.info("guard_install_rolled_back", &[("repo", Field::id(repo_id))]);
            }
        }
    }
}
