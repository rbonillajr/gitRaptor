//! Install of the hook layer (ADR-GRD-001 § 4, ADR-GRD-007 § 1), run by the daemon loop after
//! the reserved command was authorized and audited. A transaction with one commit point (the
//! key), the journal written before and after each step, and recovery at startup.

use std::path::{Path, PathBuf};

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    GuardPlan, GuardStatus, Hook, HooksPathLevel, InstallBlocker, Permission, PriorHooks,
    ProtectionState, RefBackend,
};
use gitraptor_git::cli::GitCli;
use gitraptor_git::guard_write::{FOLDER, GuardWriteError, GuardWriter, NewFile};
use gitraptor_git::{Invoker, RefStorage, RepoReader, SystemGit};
use gitraptor_policy::guard::not_preventable;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor, DEFAULT_BRANCH, SETTINGS_PATH};
use sha2::{Digest, Sha256};

use super::constants::{Constants, DISPATCH_CONF, MANIFEST, STUB_FILE, TEMPLATE_VERSION};
use super::evaluate;
use super::journal::{FileHash, Journal, Prior, Snapshot, Stage, snapshot_path};
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
    // "Hooks only" needs the confirmed install still in place: the key is ours and the folder
    // is there. Anything else is unprotected here; telling why is US-GRD-004 (ADR-GRD-005).
    let journal = journal(&keys)
        .filter(|j| j.stage == Stage::Confirmed)
        .filter(|j| in_place(j, reader.as_ref(), common));
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

/// The confirmed install is still where it was: the repo's own `core.hooksPath` is the
/// journal's and the dispatchers folder exists.
fn in_place(journal: &Journal, reader: Option<&gitraptor_git::RepoReader>, common: &Path) -> bool {
    reader
        .and_then(gitraptor_git::RepoReader::hooks_path)
        .is_some_and(|v| v == journal.hooks_dir)
        && common.join(FOLDER).join("hooks").is_dir()
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
    })
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
    let recorded = journal(&store.guard_keys().unwrap_or_default()).map(|j| j.template);
    let upgrade = status.state == ProtectionState::HooksOnly && outdated(common, recorded);
    if status.state == ProtectionState::HooksOnly && !upgrade {
        add(InstallBlocker::AlreadyInstalled);
    } else if upgrade {
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
    let prior = if status.state == ProtectionState::HooksOnly {
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
    GuardPlan {
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

fn sha256(bytes: &[u8]) -> String {
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
            "git config --file {} core.hooksPath {value}",
            config.display()
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
pub fn install(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    now_ms: i64,
) -> Result<GuardStatus, InstallError> {
    let plan = plan(ctx, repo_id, common, store);
    if plan.blockers.is_empty() && plan.status.state == ProtectionState::HooksOnly {
        return upgrade(ctx, repo_id, common, store, now_ms);
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

/// Whether the confirmed install in place is of an older template: the journal or its
/// `dispatch.conf` names one, or a dispatcher of the current template is missing (ADR-GRD-001
/// § 8, DS-US-GRD-018 D6).
fn outdated(common: &Path, recorded: Option<u32>) -> bool {
    if recorded.is_some_and(|t| t < TEMPLATE_VERSION) {
        return true;
    }
    let folder = common.join(FOLDER);
    let template = std::fs::symlink_metadata(folder.join(DISPATCH_CONF))
        .ok()
        .filter(std::fs::Metadata::is_file)
        .and_then(|_| std::fs::read_to_string(folder.join(DISPATCH_CONF)).ok())
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

/// Upgrades a confirmed install of an older template in place (ADR-GRD-001 § 8): every file of
/// the current template is replaced atomically inside the folder the journal recorded, so the
/// repo is never without a working dispatcher, and the key is not touched. The chained prior
/// hooks and their folder are the journal's. The journal lists the new files before the writes;
/// an interrupted upgrade leaves a working mix that the next `raptor guard install` completes
/// (it is idempotent).
fn upgrade(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    now_ms: i64,
) -> Result<GuardStatus, InstallError> {
    let keys = store.guard_keys().unwrap_or_default();
    let mut journal = journal(&keys)
        .filter(|j| j.stage == Stage::Confirmed)
        .ok_or_else(|| InstallError::Failed("no confirmed journal".into()))?;
    let folder = journal
        .folder
        .ok_or_else(|| InstallError::Failed("journal without the folder".into()))?;
    let folder_files = Folder::build(ctx, repo_id, common, &journal.prior, &journal.chained)?;
    let hooks_dir = common.join(FOLDER).join("hooks");
    // Before any write: the journal lists every file that may exist afterwards.
    journal.files = folder_files.hashes();
    journal.at_ms = now_ms;
    journal.raptor = ctx.raptor.to_string_lossy().into_owned();
    let save = |store: &mut RepoStore, j: &Journal| {
        store
            .set_guard_keys(Some(Some(&j.to_json())), None, None, None)
            .map_err(|e| InstallError::Failed(format!("journal: {e:?}")))
    };
    save(store, &journal)?;
    // `dispatch.conf` and the manifest last: until then the old template's constants rule.
    let mut files = folder_files.files();
    files.sort_by_key(|f| !f.executable);
    GuardWriter::new(ctx.git, ctx.invoker)
        .replace_files(common, folder.into(), &files)
        .map_err(|e| InstallError::Failed(format!("upgrade: {e}")))?;
    journal.template = TEMPLATE_VERSION;
    save(store, &journal)?;
    verify(ctx, common, &hooks_dir).map_err(InstallError::Failed)?;
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
    if writer
        .local_hooks_path(common)
        .ok()
        .flatten()
        .is_some_and(|v| Path::new(&v) == hooks_dir)
    {
        let _ = writer.unset_hooks_path(common);
    }
    let keys = store.guard_keys().unwrap_or_default();
    if let Some(j) = journal(&keys) {
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
