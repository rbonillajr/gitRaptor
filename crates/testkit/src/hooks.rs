//! Repos with prior hooks and hook managers for the Guardrails hook layer (INF-GRD-001, ADR-GRD-001
//! § 5 and § 6). The managers are **simulated**: the files and the `core.hooksPath` each one
//! leaves, as SPIKE-GRD-001 observed them (husky 9.1.7, lefthook 2.1.16, pre-commit 4.6.2), with
//! no npm, Go or Python. Every prior hook is the same "linter": it rejects a commit whose staged
//! diff contains [`LINT_MARKER`], so a scenario can check that it rejects the same before and
//! after an install (US-GRD-002).
//!
//! Nothing here installs Guardrails: that is the code under test (US-GRD-001).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::fixture::Fixture;

/// A staged diff containing this string is rejected by every prior hook of the fixtures.
pub const LINT_MARKER: &str = "FORBIDDEN-BY-LINTER";

/// The linter body shared by every simulated prior hook (POSIX `sh`, runs with Git for Windows).
const LINT: &str = "if git diff --cached | grep -q 'FORBIDDEN-BY-LINTER'; then\n  echo 'lint: forbidden marker' >&2\n  exit 1\nfi\nexit 0\n";

/// Hook names husky 9 generates in `.husky/_` (all client hooks it knows).
pub const HUSKY_HOOKS: &[&str] = &[
    "applypatch-msg",
    "commit-msg",
    "post-applypatch",
    "post-checkout",
    "post-commit",
    "post-merge",
    "post-rewrite",
    "pre-applypatch",
    "pre-auto-gc",
    "pre-commit",
    "pre-merge-commit",
    "pre-push",
    "pre-rebase",
    "prepare-commit-msg",
];

/// What was there before Guardrails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorHooks {
    /// No hook and no `core.hooksPath`.
    None,
    /// The user's own `pre-commit` in `.git/hooks`.
    Own,
    /// husky 9: local `core.hooksPath = .husky/_` (relative), `.husky/_/*` ignored, the user's
    /// `.husky/pre-commit` tracked.
    Husky,
    /// lefthook 2: its generated `pre-commit` in `.git/hooks` and `lefthook.yml`.
    Lefthook,
    /// pre-commit (framework): its generated `pre-commit` in `.git/hooks` and
    /// `.pre-commit-config.yaml`.
    PreCommit,
    /// `core.hooksPath` in the **global** config (the fixture home), absolute, with a `pre-commit`.
    /// Not restorable at local level (ADR-GRD-001 § 4, Desinstalación 1).
    Global,
}

impl PriorHooks {
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Own,
        Self::Husky,
        Self::Lefthook,
        Self::PreCommit,
        Self::Global,
    ];

    /// Whether a prior hook rejects [`LINT_MARKER`].
    pub fn has_linter(self) -> bool {
        self != Self::None
    }
}

/// A configuration that makes worktree coverage impossible to guarantee (ADR-GRD-001 § 5,
/// SPIKE-GRD-001 W03–W07): the install must refuse with these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageBlocker {
    /// `extensions.worktreeConfig` with `core.hooksPath` in a linked worktree's `config.worktree`.
    WorktreeConfig,
    /// A local `include.path` whose file sets `core.hooksPath`.
    Include,
    /// A local `includeIf "onbranch:…"`.
    IncludeIfOnbranch,
}

impl CoverageBlocker {
    pub const ALL: [Self; 3] = [Self::WorktreeConfig, Self::Include, Self::IncludeIfOnbranch];
}

/// Write an executable script (0755 on unix; Git for Windows runs it through `sh`).
pub fn write_script(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

impl Fixture {
    /// A repo with one commit and the given prior hooks.
    pub fn with_prior_hooks(git: &Path, prior: PriorHooks) -> Self {
        let f = Self::with_commit(git);
        f.add_prior_hooks(prior);
        f
    }

    /// Add the prior hooks of `prior` to the repo (and the home, for [`PriorHooks::Global`]).
    pub fn add_prior_hooks(&self, prior: PriorHooks) {
        let hooks = self.repo.join(".git/hooks");
        match prior {
            PriorHooks::None => {}
            PriorHooks::Own => write_script(
                &hooks.join("pre-commit"),
                &format!("#!/bin/sh\n# my own linter\n{LINT}"),
            ),
            PriorHooks::Husky => {
                let underscore = self.repo.join(".husky/_");
                write_script(
                    &underscore.join("h"),
                    "#!/usr/bin/env sh\nn=$(basename \"$0\")\ns=$(dirname \"$(dirname \"$0\")\")/$n\n[ ! -f \"$s\" ] && exit 0\nsh -e \"$s\" \"$@\"\n",
                );
                for name in HUSKY_HOOKS {
                    write_script(
                        &underscore.join(name),
                        "#!/usr/bin/env sh\n. \"$(dirname \"$0\")/h\"\n",
                    );
                }
                std::fs::write(underscore.join(".gitignore"), "*\n").unwrap();
                write_script(&self.repo.join(".husky/pre-commit"), LINT);
                self.git(&["add", ".husky/pre-commit"]);
                self.git(&["commit", "-q", "--no-verify", "-m", "husky"]);
                self.git(&["config", "core.hooksPath", ".husky/_"]);
            }
            PriorHooks::Lefthook => {
                self.write(
                    "lefthook.yml",
                    "pre-commit:\n  commands:\n    lint:\n      run: ./lint.sh\n",
                );
                self.git(&["add", "lefthook.yml"]);
                self.git(&["commit", "-q", "--no-verify", "-m", "lefthook"]);
                write_script(
                    &hooks.join("pre-commit"),
                    &format!("#!/bin/sh\n# lefthook (simulated by gitraptor-testkit)\n{LINT}"),
                );
            }
            PriorHooks::PreCommit => {
                self.write(
                    ".pre-commit-config.yaml",
                    "repos:\n  - repo: local\n    hooks:\n      - id: lint\n        name: lint\n        entry: ./lint.sh\n        language: system\n",
                );
                self.git(&["add", ".pre-commit-config.yaml"]);
                self.git(&["commit", "-q", "--no-verify", "-m", "pre-commit"]);
                write_script(
                    &hooks.join("pre-commit"),
                    &format!(
                        "#!/usr/bin/env bash\n# File generated by pre-commit: https://pre-commit.com\n# (simulated by gitraptor-testkit)\n{LINT}"
                    ),
                );
            }
            PriorHooks::Global => {
                let dir = self.home.join(".config/git/hooks");
                write_script(&dir.join("pre-commit"), &format!("#!/bin/sh\n{LINT}"));
                self.git(&[
                    "config",
                    "--global",
                    "core.hooksPath",
                    dir.to_str().unwrap(),
                ]);
            }
        }
    }

    /// Add a configuration that blocks the install. Creates a linked worktree for
    /// [`CoverageBlocker::WorktreeConfig`].
    pub fn add_coverage_blocker(&self, blocker: CoverageBlocker) {
        match blocker {
            CoverageBlocker::WorktreeConfig => {
                self.git(&["branch", "wt-config"]);
                let wt = self.add_worktree("cfg", "wt-config");
                self.git(&["config", "extensions.worktreeConfig", "true"]);
                self.git_in(&wt, &["config", "--worktree", "core.hooksPath", "wt-hooks"]);
            }
            CoverageBlocker::Include => {
                let inc = self.repo.join(".git/hooks.inc");
                std::fs::write(&inc, "[core]\n\thooksPath = included-hooks\n").unwrap();
                self.git(&["config", "include.path", inc.to_str().unwrap()]);
            }
            CoverageBlocker::IncludeIfOnbranch => {
                let inc = self.repo.join(".git/onbranch.inc");
                std::fs::write(&inc, "[user]\n\tname = Branch\n").unwrap();
                self.git(&[
                    "config",
                    "includeIf.onbranch:main.path",
                    inc.to_str().unwrap(),
                ]);
            }
        }
    }

    /// Stage a change containing [`LINT_MARKER`] and try to commit it **with hooks**. Returns
    /// whether the commit was rejected; a rejected commit leaves the change staged, so the same
    /// call can be repeated after an install.
    pub fn commit_with_marker_rejected(&self, dir: &Path) -> bool {
        let p = dir.join("lint-target.txt");
        std::fs::write(&p, format!("{LINT_MARKER}\n")).unwrap();
        self.git_in(dir, &["add", "lint-target.txt"]);
        let out = self
            .git_command(dir, &["commit", "-q", "-m", "marker"])
            .output()
            .expect("run git commit");
        if out.status.success() {
            // Undo, so the call is repeatable.
            self.git_in(dir, &["reset", "-q", "--soft", "HEAD~1"]);
            false
        } else {
            true
        }
    }

    /// The Git common dir of the repo, absolute.
    pub fn common_dir(&self) -> PathBuf {
        let out = self.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]);
        PathBuf::from(out.trim())
    }

    /// The main worktree and every linked worktree, in `git worktree list` order.
    pub fn worktrees(&self) -> Vec<PathBuf> {
        self.git(&["worktree", "list", "--porcelain"])
            .lines()
            .filter_map(|l| l.strip_prefix("worktree "))
            .map(PathBuf::from)
            .collect()
    }

    /// Effective `core.hooksPath` of every worktree, with its scope and origin
    /// (ADR-GRD-001 § 5, Enmienda 2026-10-04).
    pub fn hooks_path_state(&self) -> Vec<HooksPathAt> {
        self.worktrees()
            .into_iter()
            .map(|wt| {
                let value = effective_hooks_path(&self.git_command(&wt, &[]), &wt);
                HooksPathAt {
                    worktree: wt,
                    value,
                }
            })
            .collect()
    }
}

/// `core.hooksPath` as one worktree sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HooksPathAt {
    pub worktree: PathBuf,
    /// `None`: the key is not set at any level.
    pub value: Option<HooksPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HooksPath {
    /// `worktree`, `local`, `global`, `system` or `command`.
    pub scope: String,
    /// `file:<path>` or `command line:`.
    pub origin: String,
    pub value: String,
}

/// `git config --show-scope --show-origin --get core.hooksPath` run as `base` would run Git
/// (same program and environment) in `dir`, without command-line configuration. Exit 1 means
/// the key is absent.
fn effective_hooks_path(base: &Command, dir: &Path) -> Option<HooksPath> {
    let mut c = Command::new(base.get_program());
    for (k, v) in base.get_envs() {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    let out = c
        .args([
            "config",
            "--show-scope",
            "--show-origin",
            "--get",
            "core.hooksPath",
        ])
        .current_dir(dir)
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .output()
        .expect("run git config");
    match out.status.code() {
        Some(0) => {}
        Some(1) => return None,
        _ => panic!(
            "git config --get core.hooksPath failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        ),
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.trim_end_matches('\n').splitn(3, '\t');
    Some(HooksPath {
        scope: parts.next().unwrap_or_default().to_owned(),
        origin: parts.next().unwrap_or_default().to_owned(),
        value: parts.next().unwrap_or_default().to_owned(),
    })
}
