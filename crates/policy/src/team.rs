//! Team level of the configuration, read from committed objects (TS-GRD-001, ADR-GRD-004).
//!
//! Two sources, never the working tree:
//! - the **floor**: `.gitraptor/settings.json` of the copy of the main branch, the only source of
//!   relaxations and of the base branch (D6);
//! - the **worktree**: the same path in the commit of the operation's worktree `HEAD`, which only
//!   hardens.
//!
//! The base branch the engine and Guardrails use is the **confirmed** one; a different resolved
//! one is only a diagnostic. A floor that relaxes against the confirmed floor only hardens until
//! the developer confirms it (D7). Nothing here confirms anything: the confirmation is an input.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use gitraptor_git::{BlobRead, CommittedFile, NotRegular, ReadError, RefName, RepoReader};

use crate::settings::diagnostic::{Code, Diagnostic, Limit, Location, SourceKind};
use crate::settings::document::{Parsed, SourceStatus, parse_document};
use crate::settings::model::{Level, Operation, Settings};
use crate::settings::strict::MAX_BYTES;

/// Path of the team settings in a commit.
pub const SETTINGS_PATH: [&str; 2] = [".gitraptor", "settings.json"];

/// Base branch and main branch when nothing says otherwise (BR-CONS-006).
pub const DEFAULT_BRANCH: &str = "main";

/// The main branch of the repository and its known copy (decision 2; ADR-GRD-004 § 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainBranch {
    /// `origin` if there are several remotes, the only one if there is one; none otherwise.
    pub remote: Option<RefName>,
    /// Short name of the main branch: the target of `refs/remotes/<remote>/HEAD`, else `main`.
    pub name: RefName,
    /// The first that exists of `refs/remotes/<remote>/<name>` and `refs/heads/<name>`.
    pub copy: Option<MainCopy>,
}

/// The ref read as the copy of the main branch, and its commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainCopy {
    pub reference: RefName,
    pub commit: String,
}

/// Resolve the main branch and its copy, without network (Q12, NFR-03).
pub fn resolve_main_branch(reader: &RepoReader) -> Result<MainBranch, ReadError> {
    let remotes = reader.remote_names();
    let remote = match remotes.as_slice() {
        [only] => Some(only.clone()),
        many => many.iter().find(|r| r.as_str() == "origin").cloned(),
    };
    let default = RefName::new(DEFAULT_BRANCH).expect("valid default");
    let mut name = default;
    if let Some(remote) = &remote {
        let prefix = format!("refs/remotes/{remote}/");
        let head = RefName::new(&format!("{prefix}HEAD"))?;
        if let Some(target) = reader.symbolic_target(&head)?
            && let Some(short) = target.strip_prefix(&prefix)
            && let Ok(short) = RefName::new(short)
        {
            name = short;
        }
    }
    let mut candidates = Vec::new();
    if let Some(remote) = &remote {
        candidates.push(format!("refs/remotes/{remote}/{name}"));
    }
    candidates.push(format!("refs/heads/{name}"));
    let mut copy = None;
    for candidate in candidates {
        let reference = RefName::new(&candidate)?;
        if let Some(commit) = reader.resolve_ref(&reference)? {
            copy = Some(MainCopy { reference, commit });
            break;
        }
    }
    Ok(MainBranch { remote, name, copy })
}

/// What the developer confirmed, kept by the daemon in the per-repo store (ADR-GRD-004 § 3–4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmed {
    pub base_branch: RefName,
    pub floor: ConfirmedFloor,
}

/// The confirmed floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmedFloor {
    /// Confirmed without team settings (for example `main` at install time, D9).
    Absent,
    /// The blob id of the confirmed `.gitraptor/settings.json`.
    Blob(String),
}

/// One team source, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamSource {
    pub kind: SourceKind,
    /// Id of the blob read, if any.
    pub blob: Option<String>,
    pub parsed: Parsed,
}

impl TeamSource {
    fn absent(kind: SourceKind) -> Self {
        Self {
            kind,
            blob: None,
            parsed: Parsed::absent(),
        }
    }

    pub fn status(&self) -> SourceStatus {
        self.parsed.status
    }

    fn settings(&self) -> Option<&Settings> {
        self.parsed.applicable()
    }
}

/// Whether the base branch is the confirmed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseStatus {
    /// Confirmed by the developer: the only value of the repository.
    Confirmed,
    /// No confirmation yet: the resolved branch, marked as not confirmed (`base-unconfirmed`).
    Unconfirmed,
    /// No confirmation and the floor declares an invalid name: no base branch (Q42).
    Invalid,
}

/// The base branch both the engine (ahead/behind) and Guardrails use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseBranch {
    pub name: Option<RefName>,
    pub status: BaseStatus,
}

/// The branch the floor resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedBase {
    Valid(RefName),
    /// `engine.baseBranch` is not a valid branch name; never passed to Git.
    Invalid,
}

/// Permission of an operation, ordered by restriction: deny > ask > allow (BR-VAL-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    Allow,
    Ask,
    Deny,
}

/// Where a rule comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuleSource {
    SafeMinimum,
    Source(SourceKind),
}

/// The effective permission of one operation and every rule that produces it (BR-CALC-001).
/// No rule means the default, allow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRule {
    pub permission: Permission,
    pub sources: Vec<RuleSource>,
}

/// Effective permissions of the team level (ADR-GRD-004 § 2). The personal levels harden
/// them in the same way when US-GRP-013 reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePermissions {
    /// The safe minimum (deny force-push and the deletion of the base branch) is active.
    pub safe_minimum_active: bool,
    pub operations: BTreeMap<Operation, OperationRule>,
}

impl EffectivePermissions {
    pub fn permission(&self, op: Operation) -> Permission {
        self.operations[&op].permission
    }

    /// `true` if `self` is less restrictive than `other` anywhere (D7).
    fn relaxes(&self, other: &Self) -> bool {
        (!self.safe_minimum_active && other.safe_minimum_active)
            || Operation::ALL
                .iter()
                .any(|op| self.permission(*op) < other.permission(*op))
    }
}

/// Combine the floor and the sources that only harden (D6, BR-CONS-001).
///
/// Only a fully readable floor can turn the minimum off (D12). Hardeners never relax: their
/// `allow` lists and their `disableSafeMinimum` are not read.
pub fn combine(
    floor: Option<(SourceKind, &Settings)>,
    floor_readable: bool,
    hardeners: &[(SourceKind, &Settings)],
) -> EffectivePermissions {
    let disabled = floor_readable
        && floor
            .and_then(|(_, s)| s.permissions.as_ref())
            .and_then(|p| p.disable_safe_minimum)
            == Some(true);
    let mut rules: BTreeMap<Operation, Vec<(Permission, RuleSource)>> = BTreeMap::new();
    let mut add = |kind: SourceKind, settings: &Settings, with_allow: bool| {
        let Some(p) = &settings.permissions else {
            return;
        };
        for (list, permission) in [
            (&p.deny, Permission::Deny),
            (&p.ask, Permission::Ask),
            (&p.allow, Permission::Allow),
        ] {
            if permission == Permission::Allow && !with_allow {
                continue;
            }
            for op in list.iter().flatten() {
                rules
                    .entry(*op)
                    .or_default()
                    .push((permission, RuleSource::Source(kind)));
            }
        }
    };
    if let Some((kind, settings)) = floor {
        add(kind, settings, true);
    }
    for (kind, settings) in hardeners {
        add(*kind, settings, false);
    }
    if !disabled {
        rules
            .entry(Operation::ForcePush)
            .or_default()
            .push((Permission::Deny, RuleSource::SafeMinimum));
    }
    let operations = Operation::ALL
        .iter()
        .map(|op| {
            let candidates = rules.remove(op).unwrap_or_default();
            let permission = candidates
                .iter()
                .map(|(p, _)| *p)
                .max()
                .unwrap_or(Permission::Allow);
            let mut sources: Vec<RuleSource> = candidates
                .into_iter()
                .filter(|(p, _)| *p == permission)
                .map(|(_, s)| s)
                .collect();
            sources.sort();
            sources.dedup();
            (
                *op,
                OperationRule {
                    permission,
                    sources,
                },
            )
        })
        .collect();
    EffectivePermissions {
        safe_minimum_active: !disabled,
        operations,
    }
}

/// The team level, resolved for one worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamConfig {
    pub main_branch: MainBranch,
    /// The floor as read from the copy of the main branch.
    pub floor: TeamSource,
    /// The confirmed floor, when it differs from the floor and had to be read by id.
    pub confirmed_floor: Option<TeamSource>,
    /// The worktree source.
    pub worktree: TeamSource,
    /// The branch the floor resolves to (`engine.baseBranch`, else `main`).
    pub resolved_base: ResolvedBase,
    /// Effective permissions of the team level.
    pub permissions: EffectivePermissions,
    base: BaseBranch,
    guarded: Vec<RefName>,
    diagnostics: Vec<Diagnostic>,
}

impl TeamConfig {
    /// The one base branch of the repository, for the engine and for Guardrails alike.
    pub fn base_branch(&self) -> &BaseBranch {
        &self.base
    }

    /// Base branches Guardrails protects: the base branch plus, while a change is pending or
    /// nothing is confirmed, the other candidates (ADR-GRD-004 § 3). The engine never uses it.
    pub fn guarded_base_branches(&self) -> &[RefName] {
        &self.guarded
    }

    /// Diagnostics of the team level (main branch, base branch, confirmation).
    pub fn team_diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Every diagnostic: the team level and each source.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .chain(&self.floor.parsed.diagnostics)
            .chain(
                self.confirmed_floor
                    .iter()
                    .flat_map(|s| &s.parsed.diagnostics),
            )
            .chain(&self.worktree.parsed.diagnostics)
    }

    pub fn has(&self, code: Code) -> bool {
        self.diagnostics().any(|d| d.code == code)
    }
}

/// Default number of parsed documents the loader keeps (L-01).
pub const CACHE_CAPACITY: usize = 64;

/// The one loader of the team level. Holds a bounded LRU cache of parsed documents, keyed by
/// blob id and level, safe to share between threads.
pub struct TeamLoader {
    capacity: usize,
    cache: Mutex<VecDeque<((String, Level), Parsed)>>,
}

impl Default for TeamLoader {
    fn default() -> Self {
        Self::new(CACHE_CAPACITY)
    }
}

impl std::fmt::Debug for TeamLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeamLoader")
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl TeamLoader {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            cache: Mutex::new(VecDeque::new()),
        }
    }

    /// Number of documents in the cache.
    pub fn cached(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<((String, Level), Parsed)>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn parse_cached(&self, blob: &str, bytes: &[u8], kind: SourceKind) -> Parsed {
        let key = (blob.to_owned(), Level::Team);
        {
            let mut cache = self.lock();
            if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
                let entry = cache.remove(i).expect("index in range");
                let parsed = entry.1.for_source(kind);
                cache.push_front(entry);
                return parsed;
            }
        }
        let parsed = parse_document(bytes, Level::Team, kind);
        let mut cache = self.lock();
        cache.push_front((key, parsed.clone()));
        cache.truncate(self.capacity);
        parsed
    }

    fn committed(&self, reader: &RepoReader, commit: Option<&str>, kind: SourceKind) -> TeamSource {
        let Some(commit) = commit else {
            return TeamSource::absent(kind);
        };
        let ignored = |code| TeamSource {
            kind,
            blob: None,
            parsed: Parsed::ignored(Diagnostic::new(code, kind)),
        };
        match reader.committed_file(commit, &SETTINGS_PATH, MAX_BYTES as u64) {
            Ok(CommittedFile::Absent) => TeamSource::absent(kind),
            Ok(CommittedFile::NotRegular(NotRegular::Symlink)) => ignored(Code::Symlink),
            Ok(CommittedFile::NotRegular(NotRegular::Submodule)) => ignored(Code::Submodule),
            Ok(CommittedFile::NotRegular(NotRegular::WrongKind)) => ignored(Code::NotAFile),
            Ok(CommittedFile::TooLarge { .. }) => ignored(Code::LimitExceeded(Limit::Size)),
            Ok(CommittedFile::Blob { id, bytes }) => TeamSource {
                kind,
                parsed: self.parse_cached(&id, &bytes, kind),
                blob: Some(id),
            },
            // A missing object or an unreadable tree never falls open (SEC-GRD-17).
            Err(_) => ignored(Code::Unreadable),
        }
    }

    fn confirmed_blob(&self, reader: &RepoReader, id: &str) -> Option<TeamSource> {
        let kind = SourceKind::ConfirmedFloor;
        match reader.blob_by_id(id, MAX_BYTES as u64) {
            Ok(BlobRead::Blob { bytes }) => Some(TeamSource {
                kind,
                parsed: self.parse_cached(id, &bytes, kind),
                blob: Some(id.to_owned()),
            }),
            _ => None,
        }
    }

    /// Read the team level for the worktree `reader` was opened on.
    ///
    /// `confirmed` is what the developer confirmed (per-repo store); `None` means nothing was
    /// confirmed yet. Reads only committed objects: no write, no network, no working tree.
    pub fn load(
        &self,
        reader: &RepoReader,
        confirmed: Option<&Confirmed>,
    ) -> Result<TeamConfig, ReadError> {
        let main_branch = resolve_main_branch(reader)?;
        let floor = self.committed(
            reader,
            main_branch.copy.as_ref().map(|c| c.commit.as_str()),
            SourceKind::Floor,
        );
        let worktree = match reader.head() {
            Ok(head) => self.committed(reader, head.commit.as_deref(), SourceKind::Worktree),
            Err(_) => TeamSource {
                kind: SourceKind::Worktree,
                blob: None,
                parsed: Parsed::ignored(Diagnostic::new(Code::Unreadable, SourceKind::Worktree)),
            },
        };
        let mut diagnostics = Vec::new();
        let team = |code| Diagnostic::new(code, SourceKind::Team);

        // Floor and confirmed floor (D6, D7).
        let mut confirmed_floor = None;
        let mut hardening_floor = false;
        let effective_floor: Option<&TeamSource> = match confirmed {
            None => {
                hardening_floor = true;
                None
            }
            Some(conf) => {
                let same = match &conf.floor {
                    ConfirmedFloor::Absent => floor.blob.is_none(),
                    ConfirmedFloor::Blob(id) => floor.blob.as_deref() == Some(id.as_str()),
                };
                let c = match &conf.floor {
                    _ if same => Some(None),
                    ConfirmedFloor::Absent => {
                        Some(Some(TeamSource::absent(SourceKind::ConfirmedFloor)))
                    }
                    ConfirmedFloor::Blob(id) => self.confirmed_blob(reader, id).map(Some),
                };
                match c {
                    // The resolved floor is the confirmed one.
                    Some(None) => Some(&floor),
                    // The confirmed floor is gone: the floor only hardens.
                    None => {
                        diagnostics.push(team(Code::ConfirmedFloorMissing));
                        hardening_floor = true;
                        None
                    }
                    Some(Some(c)) => {
                        let as_floor = |s: &TeamSource| {
                            combine(
                                s.settings().map(|x| (s.kind, x)),
                                s.status() == SourceStatus::Readable,
                                &[],
                            )
                        };
                        if as_floor(&floor).relaxes(&as_floor(&c)) {
                            diagnostics.push(team(Code::FloorRelaxPending));
                            hardening_floor = true;
                            confirmed_floor = Some(c);
                            confirmed_floor.as_ref()
                        } else {
                            confirmed_floor = Some(c);
                            Some(&floor)
                        }
                    }
                }
            }
        };
        let mut hardeners: Vec<(SourceKind, &Settings)> = Vec::new();
        if hardening_floor && let Some(s) = floor.settings() {
            hardeners.push((SourceKind::Floor, s));
        }
        if let Some(s) = worktree.settings() {
            hardeners.push((SourceKind::Worktree, s));
        }
        let permissions = combine(
            effective_floor.and_then(|f| f.settings().map(|s| (f.kind, s))),
            effective_floor.is_some_and(|f| f.status() == SourceStatus::Readable),
            &hardeners,
        );

        // Keys only the floor may set have no effect in the worktree (D6).
        if let Some(wt) = worktree.settings() {
            let floor_settings = floor.settings();
            let base_of = |s: Option<&Settings>| {
                s.and_then(|s| s.engine.as_ref())
                    .and_then(|e| e.base_branch.clone())
            };
            if base_of(Some(wt)).is_some() && base_of(Some(wt)) != base_of(floor_settings) {
                diagnostics.push(
                    Diagnostic::new(Code::FloorOnlyKey, SourceKind::Worktree)
                        .at(Location::Pointer("/engine/baseBranch".into())),
                );
            }
            let disables =
                wt.permissions.as_ref().and_then(|p| p.disable_safe_minimum) == Some(true);
            if disables && permissions.safe_minimum_active {
                diagnostics.push(
                    Diagnostic::new(Code::FloorOnlyKey, SourceKind::Worktree)
                        .at(Location::Pointer("/permissions/disableSafeMinimum".into())),
                );
            }
        }

        // Base branch (ADR-GRD-004 § 3).
        let main = RefName::new(DEFAULT_BRANCH).expect("valid default");
        let floor_ignored = floor.status() == SourceStatus::Ignored;
        let resolved_base = match floor
            .settings()
            .and_then(|s| s.engine.as_ref())
            .and_then(|e| e.base_branch.as_deref())
        {
            None => ResolvedBase::Valid(main.clone()),
            Some(name) => match RefName::new(name) {
                Ok(r) if !name.starts_with("refs/") => ResolvedBase::Valid(r),
                _ => {
                    diagnostics.push(
                        Diagnostic::new(Code::InvalidBaseBranch, SourceKind::Floor)
                            .at(Location::Pointer("/engine/baseBranch".into())),
                    );
                    ResolvedBase::Invalid
                }
            },
        };
        let principal = main_branch.name.clone();
        let (base, guarded) = match (confirmed, &resolved_base) {
            (None, resolved) => {
                diagnostics.push(team(Code::BaseUnconfirmed));
                match resolved {
                    ResolvedBase::Valid(r) => (
                        BaseBranch {
                            name: Some(r.clone()),
                            status: BaseStatus::Unconfirmed,
                        },
                        vec![main.clone(), principal.clone(), r.clone()],
                    ),
                    ResolvedBase::Invalid => (
                        BaseBranch {
                            name: None,
                            status: BaseStatus::Invalid,
                        },
                        vec![main.clone(), principal.clone()],
                    ),
                }
            }
            (Some(conf), resolved) => {
                let c = conf.base_branch.clone();
                let base = BaseBranch {
                    name: Some(c.clone()),
                    status: BaseStatus::Confirmed,
                };
                let guarded = if floor_ignored {
                    // Fail-safe (§ 3.6).
                    vec![main.clone(), principal.clone(), c]
                } else {
                    match resolved {
                        ResolvedBase::Valid(r) if *r == c => vec![c],
                        ResolvedBase::Valid(r) => {
                            diagnostics.push(team(Code::BaseChangePending { invalid: false }));
                            vec![c, r.clone()]
                        }
                        ResolvedBase::Invalid => {
                            diagnostics.push(team(Code::BaseChangePending { invalid: true }));
                            vec![c, main.clone(), principal.clone()]
                        }
                    }
                };
                (base, guarded)
            }
        };
        let mut seen = Vec::new();
        for g in guarded {
            if !seen.contains(&g) {
                seen.push(g);
            }
        }

        Ok(TeamConfig {
            main_branch,
            floor,
            confirmed_floor,
            worktree,
            resolved_base,
            permissions,
            base,
            guarded: seen,
            diagnostics,
        })
    }
}

/// Whether a change at `git_path` (relative to a Git directory, `/` separators) can change the
/// team level, so the daemon re-reads it (ADR-GRP-007 "Lectura y recarga"). The working tree,
/// the index, objects alone and tags never do.
pub fn affects_team_config(git_path: &str) -> bool {
    let path = git_path.trim_start_matches("./");
    matches!(path, "config" | "packed-refs" | "HEAD")
        || path.starts_with("refs/remotes/")
        || path.starts_with("refs/heads/")
        || path
            .strip_prefix("worktrees/")
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(id, file)| !id.is_empty() && file == "HEAD")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::Permissions;

    fn settings(json: &str) -> Settings {
        parse_document(json.as_bytes(), Level::Team, SourceKind::Floor)
            .settings
            .map(|s| (*s).clone())
            .unwrap()
    }

    #[test]
    fn minimum_denies_force_push_unless_a_readable_floor_disables_it() {
        let empty = Settings::default();
        let p = combine(Some((SourceKind::Floor, &empty)), true, &[]);
        assert!(p.safe_minimum_active);
        assert_eq!(p.permission(Operation::ForcePush), Permission::Deny);
        assert_eq!(
            p.operations[&Operation::ForcePush].sources,
            [RuleSource::SafeMinimum]
        );
        assert_eq!(p.permission(Operation::Push), Permission::Allow);

        let off = settings(r#"{"permissions":{"disableSafeMinimum":true,"allow":["force-push"]}}"#);
        let p = combine(Some((SourceKind::Floor, &off)), true, &[]);
        assert!(!p.safe_minimum_active);
        assert_eq!(p.permission(Operation::ForcePush), Permission::Allow);
        // Partial floor (D12): the minimum is forced.
        let p = combine(Some((SourceKind::Floor, &off)), false, &[]);
        assert!(p.safe_minimum_active);
        assert_eq!(p.permission(Operation::ForcePush), Permission::Deny);
    }

    #[test]
    fn hardeners_only_harden() {
        let floor = settings(r#"{"permissions":{"allow":["push"],"ask":["merge"]}}"#);
        let wt = settings(
            r#"{"permissions":{"deny":["merge"],"allow":["force-push","merge"],"disableSafeMinimum":true}}"#,
        );
        let p = combine(
            Some((SourceKind::Floor, &floor)),
            true,
            &[(SourceKind::Worktree, &wt)],
        );
        assert!(p.safe_minimum_active);
        assert_eq!(p.permission(Operation::ForcePush), Permission::Deny);
        assert_eq!(p.permission(Operation::Merge), Permission::Deny);
        assert_eq!(
            p.operations[&Operation::Merge].sources,
            [RuleSource::Source(SourceKind::Worktree)]
        );
        assert_eq!(p.permission(Operation::Push), Permission::Allow);
        assert_eq!(
            p.operations[&Operation::Push].sources,
            [RuleSource::Source(SourceKind::Floor)]
        );
    }

    #[test]
    fn same_operation_in_two_lists_takes_the_maximum() {
        let s = Settings {
            permissions: Some(Permissions {
                allow: Some(vec![Operation::Rebase]),
                ask: Some(vec![Operation::Rebase]),
                ..Permissions::default()
            }),
            ..Settings::default()
        };
        let p = combine(Some((SourceKind::Floor, &s)), true, &[]);
        assert_eq!(p.permission(Operation::Rebase), Permission::Ask);
    }

    #[test]
    fn relaxation_compares_effective_decisions() {
        let deny_fp = settings(r#"{"permissions":{"deny":["force-push","push"]}}"#);
        let only_push = settings(r#"{"permissions":{"deny":["push"]}}"#);
        let floor = |s: &Settings| combine(Some((SourceKind::Floor, s)), true, &[]);
        // Dropping a deny the minimum already covers is not a relaxation.
        assert!(!floor(&only_push).relaxes(&floor(&deny_fp)));
        assert!(floor(&Settings::default()).relaxes(&floor(&only_push)));
        let off = settings(r#"{"permissions":{"deny":["push"],"disableSafeMinimum":true}}"#);
        assert!(floor(&off).relaxes(&floor(&only_push)));
        assert!(!floor(&deny_fp).relaxes(&floor(&only_push)));
    }

    #[test]
    fn reload_trigger_paths() {
        for yes in [
            "config",
            "packed-refs",
            "HEAD",
            "refs/remotes/origin/main",
            "refs/remotes/origin/HEAD",
            "refs/heads/main",
            "worktrees/feat-x/HEAD",
        ] {
            assert!(affects_team_config(yes), "{yes}");
        }
        for no in [
            "index",
            "objects/ab/cdef",
            "refs/tags/v1",
            "logs/HEAD",
            "worktrees/feat-x/index",
            "worktrees//HEAD",
            "COMMIT_EDITMSG",
        ] {
            assert!(!affects_team_config(no), "{no}");
        }
    }
}
