//! Install of the hook layer (ADR-GRD-001 § 4, ADR-GRD-007 § 1), run by the daemon loop after
//! the reserved command was authorized and audited. A transaction with one commit point (the
//! key), the journal written before and after each step, and recovery at startup.

use std::path::{Path, PathBuf};

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    GuardPlan, GuardStatus, Hook, HooksPathLevel, HooksStatus, InstallBlocker, LossCause,
    MinimumSet, MinimumSetStatus, Permission, PriorHooks, ProtectionState, RefBackend, RepairPlan,
};
use gitraptor_git::cli::GitCli;
use gitraptor_git::guard_write::{FOLDER, GuardWriteError, GuardWriter, NewFile};
use gitraptor_git::{Invoker, RefStorage, RepoReader, SystemGit};
use gitraptor_policy::guard::not_preventable;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor, DEFAULT_BRANCH, SETTINGS_PATH};
use sha2::{Digest, Sha256};

use super::constants::{Constants, DISPATCH_CONF, MANIFEST, STUB_FILE, TEMPLATE_VERSION};
use super::evaluate;
use super::health;
use super::journal::{FileHash, Journal, Prior, Snapshot, Stage, Upgrade, snapshot_path};
use super::prior::{self as prior_hooks, ChainImpossible};
use super::registry::{GuardEntry, GuardRegistry};
use crate::profile::{GuardKeys, ProfileDirs, RepoStore};

/// What the install needs from the daemon.
pub struct GuardCtx<'a> {
    pub git: &'a SystemGit,
    pub invoker: &'a Invoker,
    pub dirs: &'a ProfileDirs,
    pub instance: &'a str,
    /// The installed `raptor` (the daemon's own executable).
    pub raptor: &'a Path,
}

impl GuardCtx<'_> {
    fn stub(&self) -> Option<PathBuf> {
        Some(self.raptor.parent()?.join(STUB_FILE))
    }
}

/// Why an install did not happen.
#[derive(Debug)]
pub enum InstallError {
    /// A check failed before writing anything; recorded as the last refusal.
    Rejected(Vec<InstallBlocker>),
    /// A step failed; everything written was reverted.
    Failed(String),
}

fn permission(keys: &GuardKeys) -> Permission {
    match keys.permission.as_deref() {
        Some("granted") => Permission::Granted,
        Some("denied") => Permission::Denied,
        _ => Permission::NotAsked,
    }
}

pub(super) fn journal(keys: &GuardKeys) -> Option<Journal> {
    keys.journal.as_deref().and_then(Journal::from_json)
}

fn backend(storage: RefStorage) -> RefBackend {
    match storage {
        RefStorage::Files => RefBackend::Files,
        RefStorage::Reftable => RefBackend::Reftable,
    }
}

/// The protection status from the store (ADR-GRD-005 § 3, the part of US-GRD-001).
pub fn status(repo_id: &str, common: &Path, store: &RepoStore) -> GuardStatus {
    let keys = store.guard_keys().unwrap_or_default();
    let reader = evaluate::open(common);
    // "Hooks only" needs the confirmed install still active (ADR-GRD-005 § 1: key, folder,
    // dispatchers and binary as the journal recorded them); the check says why it is not.
    let journal = journal(&keys).filter(|j| j.stage == Stage::Confirmed);
    let health = health::check(common, journal.as_ref());
    let journal = journal.filter(|_| health.hooks.status == HooksStatus::Active);
    let storage = reader
        .as_ref()
        .map_or(RefStorage::Files, |r| r.ref_storage());
    let (bases, confirmed) = match &journal {
        Some(j) => (j.protected_bases.clone(), j.confirms_base.is_some()),
        None => (Vec::new(), false),
    };
    let permission = permission(&keys);
    GuardStatus {
        repo_id: repo_id.to_owned(),
        state: if journal.is_some() {
            ProtectionState::HooksOnly
        } else {
            ProtectionState::Unprotected
        },
        permission,
        offer: journal.is_none() && permission == Permission::NotAsked,
        protected_bases: bases.into_iter().map(Untrusted::new).collect(),
        base_confirmed: confirmed,
        not_preventable: not_preventable(backend(storage)),
        last_refusal: keys
            .last_refusal
            .as_deref()
            .and_then(|r| serde_json::from_str(r).ok())
            .unwrap_or_default(),
        misnamed_settings: reader.as_ref().map(misnamed_settings).unwrap_or_default(),
        // The daemon loop adds the pending action it holds (US-GRD-003).
        pending: None,
        hooks: Some(health.hooks),
        diagnostics: health.diagnostics,
        minimum_set: Some(MinimumSet {
            status: MinimumSetStatus::Active,
        }),
    }
}

/// The misnamed settings files of the floor and the `HEAD`, once each (ADR-GRP-007).
fn misnamed_settings(reader: &RepoReader) -> Vec<Untrusted> {
    let Ok(team) = gitraptor_policy::team::TeamLoader::default().load(reader, None) else {
        return Vec::new();
    };
    let mut files: Vec<String> = team
        .diagnostics()
        .filter(|d| d.code == gitraptor_policy::settings::Code::UnknownSettingsFile)
        .filter_map(|d| match &d.location {
            Some(gitraptor_policy::settings::Location::File(f)) => Some(f.clone()),
            _ => None,
        })
        .collect();
    files.sort();
    files.dedup();
    files.into_iter().map(Untrusted::new).collect()
}

/// The worktrees of the repo, the main one first (`git worktree list`).
fn worktrees(ctx: &GuardCtx<'_>, common: &Path) -> Result<Vec<PathBuf>, String> {
    let cli = GitCli::new(ctx.git, ctx.invoker, common).map_err(|e| format!("{e:?}"))?;
    Ok(cli
        .worktree_list()
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .filter(|w| !w.bare && !w.prunable)
        .map(|w| w.path)
        .collect())
}

/// The base branch the install confirms and the bases the minimum protects (Q-GRD-23,
/// ADR-GRD-004 § 3.5): without a team configuration in the copy of the main branch, `main`;
/// with one, nothing confirmed and the union {`main`, main branch}.
fn bases(common: &Path) -> (Option<String>, Vec<String>) {
    let Some(reader) = evaluate::open(common) else {
        return (None, vec![DEFAULT_BRANCH.to_owned()]);
    };
    let team = gitraptor_policy::team::resolve_main_branch(&reader)
        .ok()
        .and_then(|m| m.copy)
        .and_then(|copy| reader.committed_file(&copy.commit, &SETTINGS_PATH, 1).ok())
        .is_some_and(|f| !matches!(f, gitraptor_git::CommittedFile::Absent));
    if team {
        (None, evaluate::default_bases(&reader))
    } else {
        (
            Some(DEFAULT_BRANCH.to_owned()),
            vec![DEFAULT_BRANCH.to_owned()],
        )
    }
}

fn constants(ctx: &GuardCtx<'_>, repo_id: &str, common: &Path, prior: &str) -> Option<Constants> {
    Some(Constants {
        template: TEMPLATE_VERSION,
        raptor: ctx.raptor.to_path_buf(),
        repo: repo_id.to_owned(),
        common: gitraptor_policy::guard::fastpath::simplified(common.to_path_buf()),
        channel: ctx.dirs.runtime.clone()?,
        instance: ctx.instance.to_owned(),
        state: ctx.dirs.state.clone(),
        prior: prior.to_owned(),
        git_sh: git_sh(&ctx.git.path),
    })
}

/// Windows: the `sh` of the Git for Windows the validated `git.exe` belongs to
/// (`<root>\usr\bin\sh.exe`, or `<root>\bin\sh.exe`). The root is read from the layouts of
/// Git for Windows (`<root>\cmd`, `<root>\<mingw64|ucrt64|clangarm64|mingw32>\bin`,
/// `<root>\bin`), never from folders above it, and the `sh` must pass the same owner and DACL
/// checks as `git.exe` (SEC-10). It is a constant of the install, so the dispatcher trusts a file
/// written once and not its environment (ADR-GRD-001 § 2). Empty when there is none.
fn git_sh(git: &Path) -> String {
    if !cfg!(windows) {
        return String::new();
    }
    // The resolved path may carry the verbatim prefix, which the checks below refuse.
    let git = gitraptor_policy::guard::fastpath::simplified(git.to_path_buf());
    let Some(root) = git_root(&git) else {
        return String::new();
    };
    ["usr/bin/sh.exe", "bin/sh.exe"]
        .into_iter()
        .find_map(|rel| gitraptor_git::resolve::check_executable(&root.join(rel)).ok())
        .map(gitraptor_policy::guard::fastpath::simplified)
        .map(|sh| sh.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The root of a Git for Windows from the path of its `git.exe`, by its known layouts.
fn git_root(git: &Path) -> Option<&Path> {
    let dir = git.parent()?;
    let name = |p: &Path| p.file_name()?.to_str().map(str::to_ascii_lowercase);
    match name(dir)?.as_str() {
        "cmd" => dir.parent(),
        "bin" => {
            let up = dir.parent()?;
            match name(up)?.as_str() {
                "mingw64" | "ucrt64" | "clangarm64" | "mingw32" => up.parent(),
                _ => Some(up),
            }
        }
        _ => None,
    }
}

fn level_text(level: HooksPathLevel) -> &'static str {
    match level {
        HooksPathLevel::None => "none",
        HooksPathLevel::Local => "local",
        HooksPathLevel::Global => "global",
        HooksPathLevel::System => "system",
    }
}

/// The `prior` constant of an install: the journal's, or `<common>/hooks` for an install of
/// US-GRD-001 that recorded none.
fn prior_dir(prior: &Prior, common: &Path) -> String {
    if prior.dir.is_empty() {
        common.join("hooks").to_string_lossy().into_owned()
    } else {
        prior.dir.clone()
    }
}

/// The dispatchers of an install: the governed ones of the template, then one chain-only
/// dispatcher per other prior hook (ADR-GRD-001 § 2, conjunto mínimo + hooks previos).
fn dispatcher_names(chained: &[String]) -> Vec<String> {
    let mut names: Vec<String> = Hook::ALL.iter().map(|h| h.git_name().to_owned()).collect();
    for name in chained {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    names
}

/// Prior hooks that need a dispatcher of their own: every one outside the governed set.
fn chain_only(hooks: &[String]) -> Vec<String> {
    hooks
        .iter()
        .filter(|h| !Hook::ALL.iter().any(|g| g.git_name() == h.as_str()))
        .cloned()
        .collect()
}

/// Checks that write nothing (ADR-GRD-001 § 4 paso 1).
pub fn plan(ctx: &GuardCtx<'_>, repo_id: &str, common: &Path, store: &RepoStore) -> GuardPlan {
    let status = status(repo_id, common, store);
    let mut blockers = Vec::new();
    let mut add = |b: InstallBlocker| {
        if !blockers.contains(&b) {
            blockers.push(b);
        }
    };
    let recorded = journal(&store.guard_keys().unwrap_or_default());
    let upgrade = status.state == ProtectionState::HooksOnly && outdated(common, recorded.as_ref());
    // An install of ours that stopped being active: installing again repairs it (US-GRD-004).
    let stale = stale_install(store, &status);
    if status.state == ProtectionState::HooksOnly && !upgrade {
        add(InstallBlocker::AlreadyInstalled);
    } else if upgrade || stale.is_some() {
        // An older template of our own install (ADR-GRD-001 § 8): the key and the folder are
        // ours, so only what the files themselves need is checked.
    } else if std::fs::symlink_metadata(common.join(FOLDER)).is_ok() {
        // A folder left without its key (or never ours): adopting or removing it is US-GRD-003.
        add(InstallBlocker::OrphanFolder);
    }
    if ctx.dirs.runtime.is_none() {
        add(InstallBlocker::PlatformUnsupported);
    }
    if !ctx.stub().is_some_and(|s| s.is_file()) {
        add(InstallBlocker::DispatcherMissing);
    }
    let reader = evaluate::open(common);
    if reader.as_ref().is_some_and(|r| r.is_bare()) {
        add(InstallBlocker::Bare);
    }
    let trees = worktrees(ctx, common).unwrap_or_default();
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    for tree in trees.iter().filter(|_| !upgrade) {
        match writer.hooks_path_entries(tree) {
            Ok(entries) => {
                for e in &entries {
                    if e.scope == "worktree" {
                        add(InstallBlocker::WorktreeConfig);
                    }
                    let own_config = e.origin.as_ref().and_then(|o| o.canonicalize().ok())
                        == common.join("config").canonicalize().ok();
                    if (e.scope == "local" || e.scope == "worktree") && !own_config {
                        add(InstallBlocker::IncludeDefinesHooksPath);
                    }
                }
            }
            // Unknown: never install blind.
            Err(_) => add(InstallBlocker::ChainImpossible),
        }
        if writer.has_onbranch_include(tree).unwrap_or(true) {
            add(InstallBlocker::IncludeIfOnbranch);
        }
    }
    // The hooks the repo already had: kept and chained (US-GRD-002), or nothing installed.
    // An installed repo's own key is ours: there is nothing prior to read (US-GRD-002).
    let key_changed = stale
        .as_ref()
        .is_some_and(|(_, cause)| *cause == LossCause::HookspathChanged);
    let prior = if status.state == ProtectionState::HooksOnly || (stale.is_some() && !key_changed) {
        None
    } else {
        match prior_hooks::detect(&writer, common, &trees) {
            Ok(prior) => Some(prior),
            Err(ChainImpossible) => {
                add(InstallBlocker::ChainImpossible);
                None
            }
        }
    };
    let prior_dir = prior.as_ref().map_or(String::new(), |p| p.dir.clone());
    if constants(ctx, repo_id, common, &prior_dir).is_some_and(|c| c.render().is_err()) {
        add(InstallBlocker::NotRepresentable);
    }
    let storage = reader
        .as_ref()
        .map_or(RefStorage::Files, |r| r.ref_storage());
    let (confirms, protected) = bases(common);
    // What a repair writes: the files of the folder as it will be (with the hooks the other
    // tool's key chains), never only the old list.
    let repair = stale.as_ref().map(|(journal, cause)| {
        let (next_prior, next_chained) = match (&prior, key_changed) {
            (Some(found), true) => (
                Prior {
                    value: found.value.clone(),
                    level: level_text(found.level).to_owned(),
                    dir: found.dir.clone(),
                },
                chain_only(&found.hooks),
            ),
            _ => (journal.prior.clone(), journal.chained.clone()),
        };
        let paths = Folder::build(ctx, repo_id, common, &next_prior, &next_chained)
            .map(|f| f.paths)
            .unwrap_or_else(|_| journal.files.iter().map(|f| f.path.clone()).collect());
        if repair_conflicts(common, journal, &paths) {
            add(InstallBlocker::OrphanFolder);
        }
        RepairPlan {
            cause: *cause,
            files: paths.into_iter().map(Untrusted::new).collect(),
            // What the repair will chain: the value the same detection finds, at any level.
            chains: prior
                .as_ref()
                .filter(|_| key_changed)
                .and_then(|found| found.value.clone())
                .map(Untrusted::new),
        }
    });
    GuardPlan {
        repair,
        repo_id: repo_id.to_owned(),
        common_dir: Untrusted::from_os(common.as_os_str()),
        worktrees: trees
            .iter()
            .map(|t| Untrusted::from_os(t.as_os_str()))
            .collect(),
        hooks: Hook::ALL.to_vec(),
        hooks_dir: Untrusted::from_os(common.join(FOLDER).join("hooks").as_os_str()),
        backend: backend(storage),
        not_preventable: not_preventable(backend(storage)),
        confirms_base: confirms.map(Untrusted::new),
        protected_bases: protected.into_iter().map(Untrusted::new).collect(),
        blockers,
        status,
        prior: prior.map(|p| PriorHooks {
            hooks_path: p.value.map(Untrusted::new),
            level: p.level,
            dir: Untrusted::new(p.dir),
            hooks: p.hooks.into_iter().map(Untrusted::new).collect(),
        }),
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn manifest(
    constants: &Constants,
    hooks_dir: &Path,
    files: &[String],
    prior: &Prior,
    chained: &[String],
) -> String {
    let config = constants.common.join("config");
    let restore_key = match (prior.level.as_str(), &prior.value) {
        ("local", Some(value)) => format!(
            "git config --file {} core.hooksPath '{}'",
            config.display(),
            value.replace('\'', r"'\''")
        ),
        _ => format!(
            "git config --file {} --unset core.hooksPath",
            config.display()
        ),
    };
    serde_json::to_string_pretty(&serde_json::json!({
        "note": "GitRaptor Guardrails: recovery data only. The authoritative record is in the GitRaptor profile.",
        "template": constants.template,
        "raptor": constants.raptor,
        "hooks_dir": hooks_dir,
        "files": files,
        "prior_hooks_path": { "value": prior.value, "level": prior.level },
        "prior_hooks_dir": constants.prior,
        "chained": chained,
        "manual_revert": [
            restore_key,
            format!("remove the folder {}", constants.common.join(FOLDER).display()),
        ],
    }))
    .unwrap_or_default()
}

/// Everything the folder holds: the paths in writing order, and their content.
struct Folder {
    paths: Vec<String>,
    conf: String,
    manifest: String,
    stub: Vec<u8>,
}

impl Folder {
    fn build(
        ctx: &GuardCtx<'_>,
        repo_id: &str,
        common: &Path,
        prior: &Prior,
        chained: &[String],
    ) -> Result<Self, InstallError> {
        let constants = constants(ctx, repo_id, common, &prior_dir(prior, common))
            .ok_or_else(|| InstallError::Rejected(vec![InstallBlocker::PlatformUnsupported]))?;
        let conf = constants
            .render()
            .map_err(|_| InstallError::Rejected(vec![InstallBlocker::NotRepresentable]))?;
        let stub_path = ctx
            .stub()
            .ok_or_else(|| InstallError::Rejected(vec![InstallBlocker::DispatcherMissing]))?;
        let stub =
            std::fs::read(&stub_path).map_err(|e| InstallError::Failed(format!("stub: {e}")))?;
        let hooks_dir = common.join(FOLDER).join("hooks");
        let mut paths: Vec<String> = dispatcher_names(chained)
            .iter()
            .map(|h| format!("hooks/{h}"))
            .collect();
        paths.push(DISPATCH_CONF.to_owned());
        paths.push(MANIFEST.to_owned());
        let manifest = manifest(&constants, &hooks_dir, &paths, prior, chained);
        Ok(Self {
            paths,
            conf,
            manifest,
            stub,
        })
    }

    fn content(&self, path: &str) -> &[u8] {
        match path {
            DISPATCH_CONF => self.conf.as_bytes(),
            MANIFEST => self.manifest.as_bytes(),
            _ => &self.stub,
        }
    }

    fn hashes(&self) -> Vec<FileHash> {
        self.paths
            .iter()
            .map(|p| FileHash {
                path: p.clone(),
                sha256: sha256(self.content(p)),
            })
            .collect()
    }

    fn files(&self) -> Vec<NewFile<'_>> {
        self.paths
            .iter()
            .map(|p| NewFile {
                path: p,
                bytes: self.content(p),
                executable: p.starts_with("hooks/"),
            })
            .collect()
    }
}

/// Installs the hook layer in an observed repo. The permission is granted by the call itself
/// (the reserved command, ADR-GRD-007 § 1).
///
/// `may_repair` is whether the caller was shown the repair of a protection that stopped being
/// active (`guard.protection`): without it, installing over one is refused as it always was,
/// never done without the screen that says what changes (US-GRD-004, D9).
pub fn install(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    now_ms: i64,
    may_repair: bool,
) -> Result<GuardStatus, InstallError> {
    let plan = plan(ctx, repo_id, common, store);
    if !may_repair && plan.repair.is_some() {
        let blockers = vec![InstallBlocker::OrphanFolder];
        let refusal = serde_json::to_string(&blockers).unwrap_or_default();
        let _ = store.set_guard_keys(None, None, Some(Some(&refusal)), None);
        return Err(InstallError::Rejected(blockers));
    }
    if plan.blockers.is_empty() && plan.status.state == ProtectionState::HooksOnly {
        return upgrade(ctx, repo_id, common, store, registry, now_ms);
    }
    if plan.blockers.is_empty()
        && let Some((journal, cause)) = stale_install(store, &plan.status)
    {
        return repair(
            ctx, repo_id, common, store, registry, now_ms, journal, cause,
        );
    }
    if !plan.blockers.is_empty() {
        let refusal = serde_json::to_string(&plan.blockers).unwrap_or_default();
        let _ = store.set_guard_keys(None, None, Some(Some(&refusal)), None);
        return Err(InstallError::Rejected(plan.blockers));
    }
    // The plan found them; read once more from the same place for the journal.
    let trees = worktrees(ctx, common).map_err(InstallError::Failed)?;
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    let found = prior_hooks::detect(&writer, common, &trees)
        .map_err(|_| InstallError::Rejected(vec![InstallBlocker::ChainImpossible]))?;
    let prior = Prior {
        value: found.value.clone(),
        level: level_text(found.level).to_owned(),
        dir: found.dir.clone(),
    };
    let chained = chain_only(&found.hooks);
    let folder_files = Folder::build(ctx, repo_id, common, &prior, &chained)?;
    let hooks_dir = common.join(FOLDER).join("hooks");
    let (confirms_base, protected_bases) = bases(common);
    let mut journal = Journal {
        version: Journal::VERSION,
        stage: Stage::Installing,
        at_ms: now_ms,
        common_dir: common.to_string_lossy().into_owned(),
        hooks_dir: hooks_dir.to_string_lossy().into_owned(),
        files: folder_files.hashes(),
        folder: None,
        config: None,
        raptor: ctx.raptor.to_string_lossy().into_owned(),
        template: TEMPLATE_VERSION,
        instance: ctx.instance.to_owned(),
        prior,
        chained,
        confirms_base: confirms_base.clone(),
        protected_bases: protected_bases.clone(),
        upgrade: None,
    };
    let save = |store: &mut RepoStore, j: &Journal| {
        store
            .set_guard_keys(Some(Some(&j.to_json())), None, None, None)
            .map_err(|e| InstallError::Failed(format!("journal: {e:?}")))
    };
    // Before any write: the journal says what may exist.
    save(store, &journal)?;
    let files = folder_files.files();
    let paths = &folder_files.paths;
    let revert = |store: &mut RepoStore, why: String| -> InstallError {
        let listed: Vec<&str> = paths.iter().map(String::as_str).collect();
        rollback(&writer, common, &hooks_dir, &listed, store);
        InstallError::Failed(why)
    };
    let folder = match writer.write_folder(common, &files) {
        Ok(id) => id,
        Err(GuardWriteError::Exists) => {
            let _ = store.set_guard_keys(Some(None), None, None, None);
            return Err(InstallError::Rejected(vec![InstallBlocker::OrphanFolder]));
        }
        Err(e) => return Err(revert(store, format!("folder: {e}"))),
    };
    // Windows (M-07): no ACE of write for `Everyone`, `Users` or `Authenticated Users` over the
    // folder the dispatchers live in, checked on what was written, not on what was meant.
    #[cfg(windows)]
    if let Err(e) = gitraptor_winsys::acl::verify_private_dir(&common.join(FOLDER)) {
        return Err(revert(store, format!("folder acl: {e:?}")));
    }
    // What was verified is the folder this install wrote, not another one put in its place.
    #[cfg(windows)]
    if !matches!(writer.folder_id(common), Ok(Some(id)) if id == folder) {
        return Err(revert(store, "folder changed after it was written".into()));
    }
    journal.folder = Some(folder.into());
    save(store, &journal)?;
    // The commit point.
    if let Err(e) = writer.set_hooks_path(common, &hooks_dir) {
        return Err(revert(store, format!("key: {e}")));
    }
    match writer.config_id(common) {
        Ok(Some(id)) => journal.config = Some(id.into()),
        Ok(None) | Err(_) => return Err(revert(store, "config is not a regular file".into())),
    }
    // Past the commit point, any failure is undone here, not left to the next start.
    if save(store, &journal).is_err() {
        return Err(revert(store, "journal after the key".into()));
    }
    if let Err(why) = verify(ctx, common, &hooks_dir) {
        return Err(revert(store, why));
    }
    if let Err(why) = confirm(ctx, repo_id, store, registry, journal) {
        return Err(revert(store, why));
    }
    Ok(status(repo_id, common, store))
}

/// The confirmed install of ours whose layer stopped being active, with why (US-GRD-004): what
/// installing again repairs.
pub(crate) fn stale_install(
    store: &RepoStore,
    status: &GuardStatus,
) -> Option<(Journal, LossCause)> {
    // A repo that moved is not repaired here: its profile entry is another one (adopting or
    // removing it is US-GRD-003, E5).
    let cause = status
        .hooks
        .as_ref()
        .filter(|h| h.status == HooksStatus::Inactive)
        .and_then(|h| h.cause)
        .filter(|c| *c != LossCause::RepoMoved)?;
    let journal =
        journal(&store.guard_keys().unwrap_or_default()).filter(|j| j.stage == Stage::Confirmed)?;
    Some((journal, cause))
}

/// Whether a file the repair would write already exists in the folder and is not one the
/// journal lists: someone else's, which a repair never replaces.
fn repair_conflicts(common: &Path, journal: &Journal, paths: &[String]) -> bool {
    let folder = common.join(FOLDER);
    paths.iter().any(|p| {
        !journal.files.iter().any(|f| f.path == *p)
            && std::fs::symlink_metadata(folder.join(p)).is_ok()
    })
}

/// Installs again over an install of ours that stopped being active (US-GRD-004, D9): the
/// same installation, not a second one. The journal lists the files before they are written
/// and stays `confirmed`, so an interruption leaves the protection as inactive as it was (and
/// the next `raptor guard install` completes it, idempotently) instead of being undone at
/// startup. When another tool changed `core.hooksPath`, its value is read like any prior one:
/// chained, and restored if the protection is removed later. A file of the folder that someone
/// edited is written again: that is what the developer asked for after seeing the plan.
#[allow(clippy::too_many_arguments)]
fn repair(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    now_ms: i64,
    mut journal: Journal,
    cause: LossCause,
) -> Result<GuardStatus, InstallError> {
    use super::cut::{When, trip};
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    let key_changed = cause == LossCause::HookspathChanged;
    if key_changed {
        let trees = worktrees(ctx, common).map_err(InstallError::Failed)?;
        let found = prior_hooks::detect(&writer, common, &trees)
            .map_err(|_| InstallError::Rejected(vec![InstallBlocker::ChainImpossible]))?;
        journal.prior = Prior {
            value: found.value.clone(),
            level: level_text(found.level).to_owned(),
            dir: found.dir.clone(),
        };
        journal.chained = chain_only(&found.hooks);
    }
    let folder_files = Folder::build(ctx, repo_id, common, &journal.prior, &journal.chained)?;
    let hooks_dir = common.join(FOLDER).join("hooks");
    let save = |store: &mut RepoStore, j: &Journal| {
        store
            .set_guard_keys(Some(Some(&j.to_json())), None, None, None)
            .map_err(|e| InstallError::Failed(format!("journal: {e:?}")))
    };
    // A file of the new folder that already exists and is not ours is never replaced.
    if repair_conflicts(common, &journal, &folder_files.paths) {
        return Err(InstallError::Rejected(vec![InstallBlocker::OrphanFolder]));
    }
    // Before any write: the journal lists every file that may exist afterwards, the ones of the
    // old install that the new folder does not have included (removed below, only if untouched).
    let old_files = std::mem::take(&mut journal.files);
    let pending = journal.upgrade.take().map(|u| u.files).unwrap_or_default();
    let leftovers: Vec<FileHash> = old_files
        .into_iter()
        .chain(pending)
        .filter(|f| !folder_files.paths.contains(&f.path))
        .collect();
    journal.files = folder_files.hashes();
    journal.files.extend(leftovers.iter().cloned());
    journal.at_ms = now_ms;
    journal.raptor = ctx.raptor.to_string_lossy().into_owned();
    journal.template = TEMPLATE_VERSION;
    save(store, &journal)?;
    let mut files = folder_files.files();
    files.sort_by_key(|f| !f.executable);
    trip("repair-files", When::Before);
    // The folder may have been replaced by an identical copy (an archive restored, a `cp`): the
    // developer confirmed this repair after seeing its plan, so the folder as it is now is the
    // one the journal follows from here (a link or a non-folder is never adopted).
    if let Ok(Some(now)) = writer.folder_id(common) {
        journal.folder = Some(now.into());
    }
    if common.join(FOLDER).is_dir()
        && let Some(folder) = journal.folder
    {
        // What a killed write left next to a listed file, before the files are written again.
        writer
            .remove_file_temporaries(common, folder.into(), &journal.listed())
            .map_err(|e| InstallError::Failed(format!("repair temporaries: {e}")))?;
        writer
            .replace_files(common, folder.into(), &files)
            .map_err(|e| InstallError::Failed(format!("repair: {e}")))?;
    } else {
        match writer.write_folder(common, &files) {
            Ok(id) => journal.folder = Some(id.into()),
            Err(GuardWriteError::Exists) => {
                return Err(InstallError::Rejected(vec![InstallBlocker::OrphanFolder]));
            }
            Err(e) => return Err(InstallError::Failed(format!("repair folder: {e}"))),
        }
        save(store, &journal)?;
    }
    // The dispatchers the old install had and the new folder does not (the hooks the other
    // tool's key chained before): ours and untouched, they go; an edited one stays listed.
    let removable: Vec<&str> = leftovers
        .iter()
        .filter(|f| {
            health::read_regular(&common.join(FOLDER).join(&f.path))
                .ok()
                .flatten()
                .is_some_and(|bytes| sha256(&bytes) == f.sha256)
        })
        .map(|f| f.path.as_str())
        .collect();
    if !removable.is_empty()
        && let Some(folder) = journal.folder
    {
        writer
            .remove_folder(common, &removable, folder.into())
            .map_err(|e| InstallError::Failed(format!("repair leftovers: {e}")))?;
        journal
            .files
            .retain(|f| !removable.contains(&f.path.as_str()));
        save(store, &journal)?;
    }
    trip("repair-files", When::After);
    if key_changed {
        trip("repair-key", When::Before);
        writer
            .set_hooks_path(common, &hooks_dir)
            .map_err(|e| InstallError::Failed(format!("repair key: {e}")))?;
        trip("repair-key", When::After);
    }
    match writer.config_id(common) {
        Ok(Some(id)) => journal.config = Some(id.into()),
        Ok(None) | Err(_) => {
            return Err(InstallError::Failed("config is not a regular file".into()));
        }
    }
    save(store, &journal)?;
    verify(ctx, common, &hooks_dir).map_err(InstallError::Failed)?;
    confirm(ctx, repo_id, store, registry, journal).map_err(InstallError::Failed)?;
    Ok(status(repo_id, common, store))
}

/// Whether the confirmed install in place is of an older template or has an upgrade that was
/// not confirmed: the journal or its `dispatch.conf` names an older template, or a dispatcher of
/// the current template is missing (ADR-GRD-001 § 8, DS-US-GRD-018 D6).
fn outdated(common: &Path, journal: Option<&Journal>) -> bool {
    if journal.is_some_and(|j| j.upgrade.is_some() || j.template < TEMPLATE_VERSION) {
        return true;
    }
    let folder = common.join(FOLDER);
    let template = health::read_regular(&folder.join(DISPATCH_CONF))
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|conf| {
            conf.lines()
                .find_map(|l| l.strip_prefix("template\t"))
                .and_then(|v| v.trim().parse::<u32>().ok())
        });
    template.is_some_and(|t| t < TEMPLATE_VERSION)
        || Hook::ALL.iter().any(|h| {
            std::fs::symlink_metadata(folder.join("hooks").join(h.git_name()))
                .map_or(true, |m| !m.is_file())
        })
}

/// Points the protection watch at `journal` (the integrity reference of the checks).
fn rewatch(registry: &GuardRegistry, repo_id: &str, journal: &Journal) {
    registry.protection().watch(
        repo_id,
        super::protection::Watched {
            common: std::path::PathBuf::from(&journal.common_dir),
            journal: journal.clone(),
        },
    );
}

/// Upgrades a confirmed install of an older template in place (ADR-GRD-001 § 8), as a state
/// machine whose every step leaves dispatchers that work:
///
/// 1. the journal keeps the confirmed hashes and adds the new ones as a pending upgrade;
/// 2. the executables are replaced one by one (a new binary reads the constants of the old
///    template and behaves as it did);
/// 3. each one is read back and must be the new one;
/// 4. only then `dispatch.conf` is replaced: the one atomic `rename` that changes behaviour, so
///    constants of the new template never meet a binary that rejects them;
/// 5. the manifest, a read-only check of the key, and the journal confirmed in the new template.
///
/// The key is never touched. An interrupted upgrade is completed by the next `raptor guard
/// install` (every step is idempotent).
fn upgrade(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    now_ms: i64,
) -> Result<GuardStatus, InstallError> {
    use super::cut::{When, trip};
    let keys = store.guard_keys().unwrap_or_default();
    let mut journal = journal(&keys)
        .filter(|j| j.stage == Stage::Confirmed)
        .ok_or_else(|| InstallError::Failed("no confirmed journal".into()))?;
    let folder = journal
        .folder
        .ok_or_else(|| InstallError::Failed("journal without the folder".into()))?;
    let folder_files = Folder::build(ctx, repo_id, common, &journal.prior, &journal.chained)?;
    let hooks_dir = common.join(FOLDER).join("hooks");
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    let save = |store: &mut RepoStore, j: &Journal| {
        store
            .set_guard_keys(Some(Some(&j.to_json())), None, None, None)
            .map_err(|e| InstallError::Failed(format!("journal: {e:?}")))
    };
    // Before any write: the journal lists every file that may exist afterwards. The confirmed
    // hashes stay the reference until the last step.
    trip("upgrade-journal", When::Before);
    let mut pending = journal.upgrade.take().map(|u| u.files).unwrap_or_default();
    for hash in folder_files.hashes() {
        if !pending.contains(&hash) {
            pending.push(hash);
        }
    }
    journal.upgrade = Some(Upgrade {
        template: TEMPLATE_VERSION,
        files: pending,
    });
    journal.at_ms = now_ms;
    save(store, &journal)?;
    rewatch(registry, repo_id, &journal);
    trip("upgrade-journal", When::After);
    writer
        .remove_file_temporaries(common, folder.into(), &journal.listed())
        .map_err(|e| InstallError::Failed(format!("upgrade temporaries: {e}")))?;
    let files = folder_files.files();
    let executables: Vec<NewFile<'_>> = files.iter().copied().filter(|f| f.executable).collect();
    let replace = |files: &[NewFile<'_>]| {
        writer
            .replace_files(common, folder.into(), files)
            .map_err(|e| InstallError::Failed(format!("upgrade: {e}")))
    };
    let (first, others) = executables.split_at(executables.len().min(1));
    trip("upgrade-first-dispatcher", When::Before);
    replace(first)?;
    trip("upgrade-first-dispatcher", When::After);
    trip("upgrade-other-dispatchers", When::Before);
    replace(others)?;
    trip("upgrade-other-dispatchers", When::After);
    // The constants of the new template are written only if every dispatcher is the new one: an
    // old binary reading them would deny every push and chain nothing.
    for (hook, expected) in folder_files
        .hashes()
        .iter()
        .filter(|h| h.path.starts_with("hooks/"))
        .map(|h| (&h.path, &h.sha256))
    {
        let current = health::read_regular(&common.join(FOLDER).join(hook))
            .ok()
            .flatten();
        if !current.is_some_and(|bytes| sha256(&bytes) == *expected) {
            return Err(InstallError::Failed(format!(
                "upgrade: {hook} is not the new dispatcher"
            )));
        }
    }
    let pick = |path: &str| -> Vec<NewFile<'_>> {
        files.iter().copied().filter(|f| f.path == path).collect()
    };
    trip("upgrade-conf", When::Before);
    replace(&pick(DISPATCH_CONF))?;
    trip("upgrade-conf", When::After);
    trip("upgrade-manifest", When::Before);
    replace(&pick(MANIFEST))?;
    trip("upgrade-manifest", When::After);
    verify(ctx, common, &hooks_dir).map_err(InstallError::Failed)?;
    trip("upgrade-commit", When::Before);
    journal.files = folder_files.hashes();
    journal.template = TEMPLATE_VERSION;
    journal.raptor = ctx.raptor.to_string_lossy().into_owned();
    journal.upgrade = None;
    journal.at_ms = now_ms;
    save(store, &journal)?;
    rewatch(registry, repo_id, &journal);
    trip("upgrade-commit", When::After);
    Ok(status(repo_id, common, store))
}

/// The effective `core.hooksPath` of every worktree is ours, local, from the common `config`
/// (ADR-GRD-001 § 4 paso 4).
fn verify(ctx: &GuardCtx<'_>, common: &Path, hooks_dir: &Path) -> Result<(), String> {
    let config = common.join("config").canonicalize().ok();
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    for tree in worktrees(ctx, common)? {
        let entries = writer
            .hooks_path_entries(&tree)
            .map_err(|e| format!("{e}"))?;
        let Some(last) = entries.last() else {
            return Err("no hooksPath in a worktree".into());
        };
        let origin = last.origin.as_ref().and_then(|o| o.canonicalize().ok());
        if last.scope != "local" || origin != config || Path::new(&last.value) != hooks_dir {
            return Err("a worktree does not see the key".into());
        }
    }
    Ok(())
}

/// The final transaction of the store: the journal confirmed, the base branch and the
/// permission together (ADR-GRD-001 § 4 paso 5); then the registry and the snapshot.
fn confirm(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    mut journal: Journal,
) -> Result<(), String> {
    journal.stage = Stage::Confirmed;
    let confirmed = match &journal.confirms_base {
        Some(base) => Some(Confirmed {
            base_branch: gitraptor_git::RefName::new(base).map_err(|e| format!("{e:?}"))?,
            floor: ConfirmedFloor::Absent,
        }),
        None => None,
    };
    store
        .set_guard_keys(
            Some(Some(&journal.to_json())),
            Some("granted"),
            Some(None),
            confirmed.as_ref(),
        )
        .map_err(|e| format!("confirm: {e:?}"))?;
    publish(ctx.dirs, repo_id, &journal, registry, confirmed);
    Ok(())
}

/// Puts a confirmed install where the channel and degraded mode see it.
pub fn publish(
    dirs: &ProfileDirs,
    repo_id: &str,
    journal: &Journal,
    registry: &GuardRegistry,
    confirmed: Option<Confirmed>,
) {
    registry.protection().watch(
        repo_id,
        super::protection::Watched {
            common: std::path::PathBuf::from(&journal.common_dir),
            journal: journal.clone(),
        },
    );
    registry.set(
        repo_id,
        GuardEntry {
            common_dir: journal.common_dir.clone(),
            bases: journal.protected_bases.clone(),
            confirmed,
        },
    );
    let _ = export_snapshot(
        &dirs.state,
        &Snapshot {
            repo_id: repo_id.to_owned(),
            common_dir: journal.common_dir.clone(),
            confirmed_base: journal.confirms_base.clone(),
            protected_bases: journal.protected_bases.clone(),
        },
    );
}

/// Writes the read-only snapshot atomically in the private state folder.
fn export_snapshot(state: &Path, snapshot: &Snapshot) -> std::io::Result<()> {
    let path = snapshot_path(state, &snapshot.repo_id);
    let dir = path.parent().unwrap_or(state);
    crate::profile::fsperm::ensure_private_dir(dir)
        .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
    let tmp = dir.join(format!("{}.tmp", snapshot.repo_id));
    let _ = std::fs::remove_file(&tmp);
    {
        use std::io::Write;
        let mut file = crate::profile::create_private_file(&tmp)
            .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
        file.write_all(&serde_json::to_vec(snapshot).unwrap_or_default())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, &path)
}

/// Undoes an install that did not reach its confirmation: the key only if it is ours, then
/// the listed files, then the journal.
pub(super) fn rollback(
    writer: &GuardWriter<'_>,
    common: &Path,
    hooks_dir: &Path,
    listed: &[&str],
    store: &mut RepoStore,
) {
    let keys = store.guard_keys().unwrap_or_default();
    let recorded = journal(&keys);
    if writer
        .local_hooks_path(common)
        .ok()
        .flatten()
        .is_some_and(|v| Path::new(&v) == hooks_dir)
    {
        // The key goes back to what the repo had (US-GRD-002): the prior local value, or none.
        let restored = match recorded
            .as_ref()
            .map(|j| (j.prior.level.as_str(), j.prior.value.as_deref()))
        {
            Some(("local", Some(value))) => writer.restore_hooks_path(common, value),
            _ => writer.unset_hooks_path(common),
        };
        if restored.is_err() {
            // Never lose the prior key (NFR-01): the journal stays, the next start retries.
            return;
        }
    }
    if let Some(j) = recorded {
        // A cut between the rename and the journal leaves no recorded identity: the folder is
        // ours only if every listed file is there with the hash the journal recorded.
        let folder = j.folder.map(Into::into).or_else(|| {
            let root = common.join(FOLDER);
            let all_ours = j.files.iter().all(|f| {
                std::fs::read(root.join(&f.path)).is_ok_and(|bytes| sha256(&bytes) == f.sha256)
            });
            all_ours
                .then(|| writer.folder_id(common).ok().flatten())
                .flatten()
        });
        if let Some(folder) = folder {
            let _ = writer.remove_folder(common, listed, folder);
        }
    }
    let _ = writer.remove_temporaries(common, listed);
    let _ = store.set_guard_keys(Some(None), None, None, None);
}

/// Records the developer's denial: GitRaptor stops offering it (BR-AUTH-002).
pub fn decline(repo_id: &str, common: &Path, store: &mut RepoStore) -> GuardStatus {
    let _ = store.set_guard_keys(None, Some("denied"), None, None);
    status(repo_id, common, store)
}

/// What recovery did with an unfinished install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    Nothing,
    /// The key was not ours: everything listed was removed; the repo is as before.
    RolledBack,
    /// The key was ours and every worktree saw it: confirmed.
    Confirmed,
    /// An uninstall had put the key back: it was finished; the repo is as before the install.
    UninstallCompleted,
    /// An uninstall stopped before the key: the protection is complete and stays.
    UninstallKept,
}

/// Startup: an install left in `installing` is completed or undone (ADR-GRD-001 § 4,
/// Recuperación); a confirmed one is published again (registry, snapshot).
pub fn recover(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
) -> Recovery {
    let keys = store.guard_keys().unwrap_or_default();
    let Some(journal) = journal(&keys) else {
        return Recovery::Nothing;
    };
    if journal.stage == Stage::Confirmed {
        let confirmed = store.confirmed_team_baseline().ok().flatten();
        publish(ctx.dirs, repo_id, &journal, registry, confirmed);
        return Recovery::Nothing;
    }
    if journal.stage == Stage::Uninstalling {
        return match super::uninstall::recover(ctx, repo_id, common, store, registry, journal) {
            super::uninstall::Recovered::Completed => Recovery::UninstallCompleted,
            super::uninstall::Recovered::Kept => Recovery::UninstallKept,
        };
    }
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    let hooks_dir = PathBuf::from(&journal.hooks_dir);
    let ours = writer
        .local_hooks_path(common)
        .ok()
        .flatten()
        .is_some_and(|v| Path::new(&v) == hooks_dir);
    let listed: Vec<String> = journal.listed().into_iter().map(str::to_owned).collect();
    if ours
        && journal.folder.is_some()
        && verify(ctx, common, &hooks_dir).is_ok()
        && confirm(ctx, repo_id, store, registry, journal).is_ok()
    {
        return Recovery::Confirmed;
    }
    let listed: Vec<&str> = listed.iter().map(String::as_str).collect();
    rollback(&writer, common, &hooks_dir, &listed, store);
    Recovery::RolledBack
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_git_root_comes_from_the_known_layouts_only() {
        let root = |p: &str| git_root(Path::new(p)).map(|r| r.to_string_lossy().replace('\\', "/"));
        assert_eq!(root("/g/Git/cmd/git.exe").as_deref(), Some("/g/Git"));
        assert_eq!(
            root("/g/Git/mingw64/bin/git.exe").as_deref(),
            Some("/g/Git")
        );
        assert_eq!(root("/g/Git/bin/git.exe").as_deref(), Some("/g/Git"));
        // A shim folder is not a Git for Windows: nothing above it is searched.
        assert_eq!(root("/ProgramData/chocolatey/lib/x/git.exe"), None);
        assert_eq!(root("/g/shims/git.exe"), None);
    }
}
