//! The hooks a repo already had (US-GRD-002, ADR-GRD-001 § 2 and § 6): where Git ran them from
//! before the install, which ones exist, and whether they can be chained without altering them
//! (BR-EDGE-002). Reads only: the configuration through the Guardrails read profile, and the
//! metadata of the prior hooks folder.

use std::path::{Path, PathBuf};

use gitraptor_api::guard::HooksPathLevel;
use gitraptor_git::guard_write::{FOLDER, GuardWriter, HooksPathEntry};

/// Every hook name of githooks(5). A prior hook with one of these names gets a dispatcher that
/// chains it; nothing else is ever chained (the stub has the same list).
pub const GIT_HOOKS: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// What the install keeps and chains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prior {
    /// The effective `core.hooksPath` before the install, as Git had it.
    pub value: Option<String>,
    pub level: HooksPathLevel,
    /// The `prior` constant of the dispatchers: the folder Git ran the hooks from. Absolute,
    /// or relative to the root of each worktree (as Git resolves it).
    pub dir: String,
    /// Executable hooks of githooks(5) found there (in any worktree when relative), sorted.
    pub hooks: Vec<String>,
}

/// The prior hooks cannot be chained without altering them (`encadenado-imposible`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainImpossible;

pub fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        #[cfg(unix)]
        Ok(m) => {
            use std::os::unix::fs::PermissionsExt;
            m.is_file() && m.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        Ok(m) => m.is_file(),
        Err(_) => false,
    }
}

/// The executable hooks of `dir` (with `.exe` on Windows; `.sample` never counts).
pub fn hooks_in(dir: &Path) -> Vec<String> {
    let mut hooks: Vec<String> = GIT_HOOKS
        .iter()
        .filter(|name| {
            is_executable(&dir.join(name))
                || (cfg!(windows) && is_executable(&dir.join(format!("{name}.exe"))))
        })
        .map(|name| (*name).to_owned())
        .collect();
    hooks.sort();
    hooks
}

/// The home of the account running the daemon, from the password database (never `HOME`).
fn account_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        nix::unistd::User::from_uid(nix::unistd::getuid())
            .ok()
            .flatten()
            .map(|u| u.dir)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Expands a value the way Git does for `core.hooksPath`, when the result is the same for
/// every run: `~/` only under the account's own home (D2). `~user` and `%(prefix)` depend on
/// the Git that runs: not chained.
fn expand(
    value: &str,
    home: Option<&Path>,
    account: Option<&Path>,
) -> Result<String, ChainImpossible> {
    if let Some(rest) = value.strip_prefix("~/") {
        let home = home.ok_or(ChainImpossible)?;
        if account.is_none_or(|a| a != home) {
            return Err(ChainImpossible);
        }
        return home
            .join(rest)
            .to_str()
            .map(str::to_owned)
            .ok_or(ChainImpossible);
    }
    if value.starts_with('~') || value.starts_with("%(") {
        return Err(ChainImpossible);
    }
    Ok(value.to_owned())
}

fn level(scope: &str) -> Option<HooksPathLevel> {
    match scope {
        "local" => Some(HooksPathLevel::Local),
        "global" => Some(HooksPathLevel::Global),
        "system" => Some(HooksPathLevel::System),
        _ => None,
    }
}

/// Whether `dir` is (or is inside) the Guardrails folder: chaining it would chain ourselves.
fn inside_guardrails(dir: &Path, common: &Path) -> bool {
    let ours = common.join(FOLDER);
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let ours_canonical = ours.canonicalize().unwrap_or(ours.clone());
    canonical.starts_with(&ours) || canonical.starts_with(&ours_canonical)
}

/// The pure part of [`detect`]: what the entries of the main worktree say.
pub fn from_entries(
    entries: &[HooksPathEntry],
    common: &Path,
    worktrees: &[PathBuf],
    home: Option<&Path>,
    account: Option<&Path>,
) -> Result<Prior, ChainImpossible> {
    let own_config = common.join("config");
    let own = |e: &HooksPathEntry| {
        e.origin.as_ref().is_some_and(|o| {
            o == &own_config
                || matches!((o.canonicalize(), own_config.canonicalize()), (Ok(a), Ok(b)) if a == b)
        })
    };
    // The uninstall writes back one value: two at local level cannot be restored.
    if entries
        .iter()
        .filter(|e| e.scope == "local" && own(e))
        .count()
        > 1
    {
        return Err(ChainImpossible);
    }
    let Some(last) = entries.last() else {
        let dir = common.join("hooks");
        return Ok(Prior {
            value: None,
            level: HooksPathLevel::None,
            hooks: hooks_in(&dir),
            dir: dir.to_str().ok_or(ChainImpossible)?.to_owned(),
        });
    };
    let level = level(&last.scope).ok_or(ChainImpossible)?;
    if level == HooksPathLevel::Local && !own(last) {
        // Defined by an include: not ours to restore (the coverage blocker says why).
        return Err(ChainImpossible);
    }
    let value = &last.value;
    if value.is_empty()
        || value.starts_with('-')
        || value.contains(['\n', '\r', '\t', '\0'])
        || value.chars().any(char::is_control)
    {
        return Err(ChainImpossible);
    }
    let dir = expand(value, home, account)?;
    let roots: Vec<PathBuf> = if Path::new(&dir).is_absolute() {
        vec![PathBuf::from(&dir)]
    } else {
        worktrees.iter().map(|t| t.join(&dir)).collect()
    };
    if roots.iter().any(|r| inside_guardrails(r, common)) {
        return Err(ChainImpossible);
    }
    let mut hooks: Vec<String> = roots.iter().flat_map(|r| hooks_in(r)).collect();
    hooks.sort();
    hooks.dedup();
    Ok(Prior {
        value: Some(value.clone()),
        level,
        dir,
        hooks,
    })
}

/// The prior hooks of the repo, read from its main worktree (ADR-GRD-001 § 5).
pub fn detect(
    writer: &GuardWriter<'_>,
    common: &Path,
    worktrees: &[PathBuf],
) -> Result<Prior, ChainImpossible> {
    let main = worktrees.first().ok_or(ChainImpossible)?;
    let entries = writer
        .hooks_path_entries(main)
        .map_err(|_| ChainImpossible)?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    from_entries(
        &entries,
        common,
        worktrees,
        home.as_deref(),
        account_home().as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(scope: &str, origin: Option<&Path>, value: &str) -> HooksPathEntry {
        HooksPathEntry {
            scope: scope.into(),
            origin: origin.map(Path::to_path_buf),
            value: value.into(),
        }
    }

    fn script(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn without_a_hooks_path_the_common_hooks_folder_is_chained() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join(".git");
        script(&common.join("hooks/pre-commit"));
        script(&common.join("hooks/post-checkout"));
        std::fs::write(common.join("hooks/pre-push.sample"), "#!/bin/sh\n").unwrap();
        let prior = from_entries(&[], &common, &[tmp.path().into()], None, None).unwrap();
        assert_eq!(prior.level, HooksPathLevel::None);
        assert_eq!(prior.value, None);
        assert_eq!(Path::new(&prior.dir), common.join("hooks"));
        assert_eq!(prior.hooks, ["post-checkout", "pre-commit"]);
    }

    #[test]
    fn a_relative_value_is_the_union_of_every_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join("main/.git");
        let config = common.join("config");
        let main = tmp.path().join("main");
        let linked = tmp.path().join("linked");
        script(&main.join(".husky/_/pre-commit"));
        script(&linked.join(".husky/_/commit-msg"));
        let entries = [entry("local", Some(&config), ".husky/_")];
        let prior = from_entries(&entries, &common, &[main, linked], None, None).unwrap();
        assert_eq!(prior.level, HooksPathLevel::Local);
        assert_eq!(prior.dir, ".husky/_");
        assert_eq!(prior.hooks, ["commit-msg", "pre-commit"]);
    }

    #[test]
    fn the_last_definition_wins_and_keeps_its_level() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join(".git");
        let global = tmp.path().join("global");
        script(&global.join("post-commit"));
        let entries = [
            entry("system", Some(Path::new("/etc/gitconfig")), "/nowhere"),
            entry(
                "global",
                Some(&tmp.path().join(".gitconfig")),
                global.to_str().unwrap(),
            ),
        ];
        let prior = from_entries(&entries, &common, &[tmp.path().into()], None, None).unwrap();
        assert_eq!(prior.level, HooksPathLevel::Global);
        assert_eq!(prior.hooks, ["post-commit"]);
    }

    #[test]
    fn what_cannot_be_chained() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join(".git");
        let config = common.join("config");
        let trees = [tmp.path().to_path_buf()];
        let local = |v: &str| vec![entry("local", Some(&config), v)];
        for entries in [
            local("hooks\tdir"),
            local("hooks\ndir"),
            local(""),
            local("-hooks"),
            local("~other/hooks"),
            local("%(prefix)/hooks"),
            local(common.join("gitraptor/hooks").to_str().unwrap()),
            local(".git/gitraptor/hooks"),
            // Two values at local level: the uninstall could not write both back.
            vec![
                entry("local", Some(&config), "a"),
                entry("local", Some(&config), "b"),
            ],
            // Defined by an include, or at worktree level.
            vec![entry("local", Some(&tmp.path().join("inc")), "a")],
            vec![entry("worktree", Some(&config), "a")],
        ] {
            assert_eq!(
                from_entries(&entries, &common, &trees, None, None),
                Err(ChainImpossible),
                "{entries:?}"
            );
        }
    }

    #[test]
    fn a_tilde_expands_only_under_the_account_home() {
        let home = Path::new("/Users/someone");
        // The separator of the platform: `C:\Users\me\hooks` on Windows.
        assert_eq!(
            expand("~/hooks", Some(home), Some(home)).unwrap(),
            home.join("hooks").to_string_lossy()
        );
        assert_eq!(
            expand("~/hooks", Some(home), Some(Path::new("/Users/other"))),
            Err(ChainImpossible)
        );
        assert_eq!(expand("~/hooks", None, Some(home)), Err(ChainImpossible));
        assert_eq!(expand("/abs/hooks", None, None).unwrap(), "/abs/hooks");
        assert_eq!(expand(".husky/_", None, None).unwrap(), ".husky/_");
    }
}
