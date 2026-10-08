//! Discovery roots and candidates in the daemon loop (US-GRP-020,
//! US-GRP-022; ADR-GRP-010, Enmienda 2026-10-07, N6). The loop is the only
//! writer of the profile: the channel asks it here once the requester is
//! authorized, and the discovery module sends it each listing.

use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use gitraptor_api::discovery::{
    CandidateView, RepoDiscoveredData, RootAddResult, RootBroadData, RootRejectedData,
    RootRejection, RootRemoveResult, RootView,
};
use gitraptor_api::event::REPO_DISCOVERED;

use super::{Daemon, now_ms};
use crate::discovery::{self, Listing, RootContext, ValidRoot};
use crate::profile::{DiscoveryCandidate, DiscoveryRoot};

/// Debug-build test hook: `GITRAPTOR_TEST_DISCOVERY_HOME` is the folder the
/// daemon takes as the developer's home for discovery (the broad `home`
/// root and its exclusions), so an end-to-end test never lists the real
/// one. Release builds do not even read it (SEC-06).
pub const DISCOVERY_HOME_ENV: &str = "GITRAPTOR_TEST_DISCOVERY_HOME";
/// Debug-build test hook: `GITRAPTOR_TEST_DISCOVERY_POLL_MS` lists every
/// root at that interval, so an end-to-end test sees a new repo without
/// waiting for the real intervals.
pub const DISCOVERY_POLL_ENV: &str = "GITRAPTOR_TEST_DISCOVERY_POLL_MS";

/// How the roots are watched and listed (N6). Every interval is an
/// ⚠️ ASSUMPTION of the ADR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryConfig {
    /// The developer's home folder; `None`: `$HOME` (`%USERPROFILE%`).
    pub home: Option<PathBuf>,
    /// After a change in a watched root, the wait before listing it.
    pub settle: Duration,
    /// A normal root where the OS gives no first-level watch: macOS, whose
    /// `notify` 8.2 backend (FSEvents) is recursive.
    pub poll: Duration,
    /// A broad root, which is never watched (RES-03).
    pub broad_poll: Duration,
    /// Every root, for missed events and clones in progress.
    pub safety: Duration,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            home: None,
            settle: Duration::from_secs(2),
            poll: Duration::from_secs(30),
            broad_poll: Duration::from_secs(60),
            safety: Duration::from_secs(300),
        }
    }
}

impl DiscoveryConfig {
    /// The real daemon's, with the debug-build test hooks.
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if cfg!(debug_assertions) {
            config.home = std::env::var_os(DISCOVERY_HOME_ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from);
            if let Some(ms) = std::env::var(DISCOVERY_POLL_ENV)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|ms| *ms > 0)
            {
                config = config.every(Duration::from_millis(ms));
            }
        }
        config
    }

    /// Every root listed at `interval` (tests).
    pub fn every(self, interval: Duration) -> Self {
        Self {
            settle: interval,
            poll: interval,
            broad_poll: interval,
            safety: interval,
            ..self
        }
    }

    pub(crate) fn home(&self) -> Option<PathBuf> {
        self.home.clone().or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|h| !h.is_empty())
                .map(PathBuf::from)
        })
    }
}

/// Why the loop refused a discovery request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) enum DiscoveryError {
    Rejected(RootRejectedData),
    Broad(RootBroadData),
    UnknownRoot,
    NotACandidate,
    /// The profile is unavailable.
    Failed,
}

/// A discovery request to the loop.
#[derive(Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) enum DiscoveryRequest {
    Roots(SyncSender<Option<Vec<RootView>>>),
    Candidates(SyncSender<Option<Vec<CandidateView>>>),
    RootAdd {
        path: PathBuf,
        confirm_broad: bool,
        reply: SyncSender<Result<RootAddResult, DiscoveryError>>,
    },
    RootRemove {
        path: PathBuf,
        reply: SyncSender<Result<RootRemoveResult, DiscoveryError>>,
    },
    Dismiss {
        path: PathBuf,
        reply: SyncSender<Result<String, DiscoveryError>>,
    },
    /// A listing of the discovery module.
    Listed { root: String, listing: Listing },
    /// The path of a candidate is no longer a repo.
    Forget(PathBuf),
}

pub(crate) fn root_view(root: &DiscoveryRoot) -> RootView {
    RootView {
        path: root.path.clone(),
        broad: root.broad,
        added_utc_ms: root.added_ms,
    }
}

pub(crate) fn candidate_view(candidate: &DiscoveryCandidate) -> CandidateView {
    CandidateView {
        name: Path::new(&candidate.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned(),
        path: candidate.path.clone(),
        root: candidate.root.clone(),
        found_utc_ms: candidate.found_ms,
    }
}

fn found_pairs(listing: &Listing) -> Vec<(String, String)> {
    listing
        .found
        .iter()
        .filter_map(|f| Some((f.path.to_str()?.to_owned(), f.key_path.clone())))
        .collect()
}

/// The canonical form of a path the developer typed, for lookups; the path
/// itself when it no longer exists.
fn lookup_path(path: &Path) -> String {
    gitraptor_git::paths::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

impl Daemon {
    pub(super) fn discovery(&mut self, request: DiscoveryRequest) {
        match request {
            DiscoveryRequest::Roots(reply) => {
                let roots = self.profile.discovery_roots().ok();
                let _ = reply.send(roots.map(|r| r.iter().map(root_view).collect()));
            }
            DiscoveryRequest::Candidates(reply) => {
                let candidates = self.profile.discovery_candidates().ok();
                let _ = reply.send(candidates.map(|c| c.iter().map(candidate_view).collect()));
            }
            DiscoveryRequest::RootAdd {
                path,
                confirm_broad,
                reply,
            } => {
                let _ = reply.send(self.discovery_root_add(&path, confirm_broad));
            }
            DiscoveryRequest::RootRemove { path, reply } => {
                let _ = reply.send(self.discovery_root_remove(&path));
            }
            DiscoveryRequest::Dismiss { path, reply } => {
                let path = lookup_path(&path);
                let answer = match self.profile.dismiss_discovery_candidate(&path, now_ms()) {
                    Ok(true) => Ok(path),
                    Ok(false) => Err(DiscoveryError::NotACandidate),
                    Err(_) => Err(DiscoveryError::Failed),
                };
                let _ = reply.send(answer);
            }
            DiscoveryRequest::Listed { root, listing } => {
                if listing.truncated {
                    self.logger.warn(
                        "discovery_root_truncated",
                        &[("entries", discovery::MAX_ENTRIES.into())],
                    );
                }
                let found = found_pairs(&listing);
                match self
                    .profile
                    .sync_discovery_candidates(&root, &found, now_ms())
                {
                    Ok(new) if !new.is_empty() => self.publish_discovered(&root, &new),
                    Ok(_) => {}
                    Err(_) => self.logger.warn("discovery_sync_failed", &[]),
                }
            }
            DiscoveryRequest::Forget(path) => {
                // The folder is gone: its parent gives the canonical form.
                let canonical = path.parent().zip(path.file_name()).and_then(|(parent, name)| {
                    Some(gitraptor_git::paths::canonicalize(parent).ok()?.join(name))
                });
                for path in [Some(path), canonical].into_iter().flatten() {
                    let _ = self
                        .profile
                        .forget_discovery_candidate(&path.to_string_lossy());
                }
            }
        }
    }

    fn root_context(&self) -> RootContext {
        let dirs = &self.config.dirs;
        let mut profile = dirs.owned_dirs().to_vec();
        profile.extend([dirs.data.clone(), dirs.config.clone(), dirs.state.clone()]);
        profile.extend(dirs.runtime.clone());
        RootContext {
            home: self.config.discovery.home(),
            profile,
        }
    }

    fn discovery_root_add(
        &mut self,
        path: &Path,
        confirm_broad: bool,
    ) -> Result<RootAddResult, DiscoveryError> {
        let ctx = self.root_context();
        let ValidRoot {
            path: root,
            broad,
            entries,
        } = discovery::validate_root(path, &ctx).map_err(DiscoveryError::Rejected)?;
        let text = root
            .to_str()
            .ok_or(DiscoveryError::Rejected(RootRejectedData {
                reason: RootRejection::Unreadable,
                real_path: None,
            }))?
            .to_owned();
        let roots = self
            .profile
            .discovery_roots()
            .map_err(|_| DiscoveryError::Failed)?;
        if let Some(existing) = roots.iter().find(|r| r.path == text) {
            let candidates = self
                .profile
                .discovery_candidates()
                .map_err(|_| DiscoveryError::Failed)?
                .iter()
                .filter(|c| c.root == text)
                .count();
            return Ok(RootAddResult {
                root: root_view(existing),
                already: true,
                candidates: u32::try_from(candidates).unwrap_or(u32::MAX),
            });
        }
        if roots.len() >= discovery::MAX_ROOTS {
            return Err(DiscoveryError::Rejected(RootRejectedData {
                reason: RootRejection::TooMany,
                real_path: None,
            }));
        }
        if let Some(reason) = broad
            && !confirm_broad
        {
            return Err(DiscoveryError::Broad(RootBroadData {
                reason,
                path: text,
                entries,
            }));
        }
        let home_root = ctx.home.as_deref().and_then(|h| gitraptor_git::paths::canonicalize(h).ok())
            == Some(root.clone());
        let listing = discovery::list_first_level(&root, home_root).unwrap_or_default();
        let now = now_ms();
        self.profile
            .add_discovery_root(&text, broad.is_some(), now)
            .map_err(|_| DiscoveryError::Failed)?;
        let new = self
            .profile
            .sync_discovery_candidates(&text, &found_pairs(&listing), now)
            .map_err(|_| DiscoveryError::Failed)?;
        self.logger.info(
            "discovery_root_added",
            &[
                ("broad", broad.is_some().into()),
                ("candidates", new.len().into()),
            ],
        );
        // One notice per declared root, with how many repos it holds.
        self.publish_discovered(&text, &new);
        self.discovery_roots_changed();
        Ok(RootAddResult {
            root: RootView {
                path: text,
                broad: broad.is_some(),
                added_utc_ms: now,
            },
            already: false,
            candidates: u32::try_from(new.len()).unwrap_or(u32::MAX),
        })
    }

    fn discovery_root_remove(&mut self, path: &Path) -> Result<RootRemoveResult, DiscoveryError> {
        let root = lookup_path(path);
        match self.profile.remove_discovery_root(&root) {
            Ok(Some(removed)) => {
                self.discovery_roots_changed();
                Ok(RootRemoveResult {
                    root,
                    candidates_removed: removed,
                })
            }
            Ok(None) => Err(DiscoveryError::UnknownRoot),
            Err(_) => Err(DiscoveryError::Failed),
        }
    }

    fn publish_discovered(&self, root: &str, new: &[DiscoveryCandidate]) {
        let data = RepoDiscoveredData {
            root: root.to_owned(),
            candidates: new.iter().map(candidate_view).collect(),
            count: u32::try_from(new.len()).unwrap_or(u32::MAX),
        };
        self.bus.publish(REPO_DISCOVERED, data, None, |_| {});
    }

    /// Tells the discovery module the roots it watches.
    pub(super) fn discovery_roots_changed(&self) {
        if let Ok(roots) = self.profile.discovery_roots() {
            self.modules.discovery_roots_changed(&roots, self.home_key());
        }
    }

    /// The canonical home folder, to recognize the home root.
    pub(super) fn home_key(&self) -> Option<PathBuf> {
        self.config
            .discovery
            .home()
            .and_then(|h| gitraptor_git::paths::canonicalize(&h).ok())
    }

    /// A repo was added: it is no longer a candidate nor dismissed.
    pub(super) fn discovered_repo_added(&mut self, common_dir: &Path) {
        if let Ok(key) = crate::profile::normalize_common_dir(common_dir) {
            let _ = self.profile.forget_discovered_key(&key.key_path);
        }
    }
}
