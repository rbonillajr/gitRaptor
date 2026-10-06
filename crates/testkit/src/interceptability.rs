//! Interceptability matrix (INF-GRD-001; ADR-GRD-002 § 1–3 and Validación 1, 2, 12, 13): run each
//! operation of the catalog (BR-VAL-002) and each declared skip with **raw Git**, on a temporary
//! repo whose `core.hooksPath` points to probe dispatchers, deny the hook that governs it, and
//! observe whether the hook ran, the exit code and what effects were left. From that observation
//! derive how the operation must be published, and compare with the reference list.
//!
//! The reference list ([`REFERENCE`]) is the content of ADR-GRD-002 § 3 verified by SPIKE-GRD-001
//! on macOS. When `crates/policy` publishes its versioned list (US-GRD-004), the comparison moves
//! to the tests of `crates/policy` and this table is deleted (one source of truth).
//!
//! The same probes count the hook processes per command ([`count_hooks`]), the deterministic cost
//! gate of ADR-GRD-002 § 5 (Enmienda 2026-10-04).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::fixture::Fixture;
use crate::hooks::write_script;

/// Client hooks Git runs for the operations of the catalog (githooks(5)); every one gets a probe.
pub const CLIENT_HOOKS: &[&str] = &[
    "applypatch-msg",
    "commit-msg",
    "post-applypatch",
    "post-checkout",
    "post-commit",
    "post-index-change",
    "post-merge",
    "post-rewrite",
    "pre-applypatch",
    "pre-auto-gc",
    "pre-commit",
    "pre-merge-commit",
    "pre-push",
    "pre-rebase",
    "prepare-commit-msg",
    "reference-transaction",
];

/// The hooks of the cost gate: [`CLIENT_HOOKS`] without `post-index-change`, whose count depends on
/// whether Git finds the index racily clean (timing, not behaviour: 9 or 10 in the same `rebase`
/// under load). Guardrails only installs it when the prior hooks had it (ADR-GRD-001 § 2).
pub const COST_HOOKS: &[&str] = &[
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
    "reference-transaction",
];

/// The mandatory dispatchers of the minimum set (ADR-GRD-001 § 2, Enmienda 2026-10-04).
pub const MINIMUM_HOOKS: &[&str] = &["pre-push", "pre-rebase", "reference-transaction"];

/// Probe dispatcher: one log line per invocation (`hook arg1`), stdin read for the hooks that
/// get one, and `exit 1` when `PROBE_DENY` names this hook (and `PROBE_DENY_ARG` its first
/// argument, and `PROBE_DENY_MATCH` a substring of its stdin). Only constants and `sh` builtins
/// plus `cat`.
const PROBE: &str = r#"#!/bin/sh
n=${0##*/}
input=''
case "$n" in pre-push|reference-transaction|post-rewrite) input=$(cat) ;; esac
printf '%s %s\n' "$n" "${1:-}" >> '@LOG@'
[ "${PROBE_DENY:-}" = "$n" ] || exit 0
[ -z "${PROBE_DENY_ARG:-}" ] || [ "$PROBE_DENY_ARG" = "${1:-}" ] || exit 0
[ -z "${PROBE_DENY_MATCH:-}" ] && { printf '%s %s DENY\n' "$n" "${1:-}" >> '@LOG@'; exit 1; }
case "$input" in *"$PROBE_DENY_MATCH"*) printf '%s %s DENY\n' "$n" "${1:-}" >> '@LOG@'; exit 1 ;; esac
exit 0
"#;

/// A Git version, `major.minor.patch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitVersion(pub u32, pub u32, pub u32);

impl GitVersion {
    /// Parse `git version 2.50.1 (Apple Git-155)` or `git version 2.47.1.windows.2`.
    pub fn parse(s: &str) -> Option<Self> {
        let v = s.trim().strip_prefix("git version ")?;
        let mut n = v
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse::<u32>());
        Some(Self(
            n.next()?.ok()?,
            n.next()?.ok()?,
            n.next().and_then(Result::ok).unwrap_or(0),
        ))
    }

    pub fn of(git: &Path) -> Self {
        let out = Command::new(git)
            .arg("--version")
            .output()
            .expect("git --version");
        Self::parse(&String::from_utf8_lossy(&out.stdout)).expect("parse git --version")
    }

    /// `extensions.refStorage=reftable` exists from Git 2.45.
    pub fn has_reftable(self) -> bool {
        self >= Self(2, 45, 0)
    }
}

impl fmt::Display for GitVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Ref storage backend of the repo under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefFormat {
    Files,
    Reftable,
}

/// The moment of ADR-GRD-002 § 1, as observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    /// The governing hook ran before any effect and its denial left nothing behind.
    A,
    /// The hook ran and denied, but partial effects remain (letters: see [`Effects`]).
    B,
    /// The governing hook did not run.
    C,
    /// The hook ran and denied, but the operation succeeded anyway.
    NotImpeded,
}

/// How an entry is published (ADR-GRD-002 § 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Published {
    Impedible,
    /// Not impedible, with its reason: `C`, `B` or `no-reconocible`.
    NotImpedible(&'static str),
    /// A voluntary skip declared in the list.
    DeclaredSkip,
}

/// What the governing hook should deny in one case.
#[derive(Debug, Clone, Copy)]
pub struct Deny {
    pub hook: &'static str,
    pub arg: Option<&'static str>,
    /// Substring of the hook's stdin (a ref name).
    pub matching: Option<&'static str>,
}

impl Deny {
    pub const fn hook(hook: &'static str) -> Self {
        Self {
            hook,
            arg: None,
            matching: None,
        }
    }

    /// `reference-transaction` in `prepared` on a line mentioning `refname` (the second line).
    pub const fn prepared(refname: &'static str) -> Self {
        Self {
            hook: "reference-transaction",
            arg: Some("prepared"),
            matching: Some(refname),
        }
    }
}

/// Environment of the case command beyond the fixture's.
type Env = &'static [(&'static str, &'static str)];

/// One row of the catalog run by the executor.
#[derive(Clone, Copy)]
pub struct Case {
    /// Code of the entry in the published list.
    pub code: &'static str,
    /// Prepare the repo (runs before the probes are installed, without hooks).
    pub setup: fn(&Lab),
    /// The Git command, run in the repo (relative paths such as `../wt` land in the lab root).
    pub argv: &'static [&'static str],
    pub env: Env,
    pub deny: Deny,
    pub refs: RefFormat,
}

/// The temporary machine of one case: the fixture plus a bare remote `remote.git`.
pub struct Lab {
    pub f: Fixture,
    pub remote: PathBuf,
    pub log: PathBuf,
    pub probes: PathBuf,
}

impl Lab {
    /// A repo (files or reftable) with `main` (2 commits), `feat` (1 commit off the first) and a
    /// bare remote `origin` holding both.
    pub fn new(git: &Path, refs: RefFormat) -> Self {
        let f = Fixture::new(git);
        if refs == RefFormat::Reftable {
            // Re-create the repo with reftable before anything is written in it.
            std::fs::remove_dir_all(f.repo.join(".git")).unwrap();
            f.git(&["init", "-q", "--ref-format=reftable"]);
        }
        f.write("a.txt", "alpha\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-q", "-m", "one"]);
        f.git(&["branch", "feat"]);
        f.write("b.txt", "beta\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-q", "-m", "two"]);
        f.git(&["checkout", "-q", "feat"]);
        f.write("c.txt", "gamma\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-q", "-m", "feat"]);
        f.git(&["checkout", "-q", "main"]);
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", "remote.git"]);
        f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        f.git(&["push", "-q", "origin", "main", "feat"]);
        let log = f.root.join("probe.log");
        let probes = f.root.join("probes");
        Self {
            f,
            remote,
            log,
            probes,
        }
    }

    /// Install a probe for each of `hooks` and point `core.hooksPath` (local, absolute) at them.
    /// Only ever on the lab's own temporary repo (NFR-01).
    pub fn install_probes(&self, hooks: &[&str]) {
        crate::guard::check(&self.f.repo).unwrap_or_else(|e| panic!("harness guard: {e}"));
        let log = self.log.to_str().unwrap().replace('\\', "/");
        let body = PROBE.replace("@LOG@", &log);
        for h in hooks {
            write_script(&self.probes.join(h), &body);
        }
        std::fs::write(&self.log, "").unwrap();
        let path = self.probes.to_str().unwrap().replace('\\', "/");
        self.f.git(&["config", "core.hooksPath", &path]);
    }

    /// Commit a change on the current branch without running hooks (setup helper).
    pub fn commit(&self, file: &str, content: &str, msg: &str) {
        self.f.write(file, content);
        self.f.git(&["add", file]);
        self.f.git(&["commit", "-q", "--no-verify", "-m", msg]);
    }

    /// Hook lines logged so far (`hook arg1`), without the `DENY` markers.
    pub fn invocations(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.ends_with(" DENY"))
            .map(|l| l.trim_end().to_owned())
            .collect()
    }

    fn denied(&self) -> bool {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .any(|l| l.ends_with(" DENY"))
    }

    /// The state an operation can affect, read without running any hook: `r` refs/heads and
    /// `HEAD` (repo and remote), `i` index, `w` working tree, `s` operation in progress and
    /// worktrees.
    pub fn effects_state(&self) -> Effects {
        let git = |args: &[&str], dir: &Path| {
            let mut full = vec!["-c", "core.hooksPath=/nonexistent-gitraptor-probe-off"];
            full.extend_from_slice(args);
            let out = self.f.git_command(dir, &full).output().expect("run git");
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        let repo = &self.f.repo;
        let refs = [
            git(&["for-each-ref", "refs/heads"], repo),
            git(&["symbolic-ref", "-q", "HEAD"], repo),
            git(&["rev-parse", "-q", "--verify", "HEAD"], repo),
            git(&["for-each-ref", "refs/heads"], &self.remote),
        ]
        .join("\n");
        let index = git(&["ls-files", "-s"], repo);
        let worktree = worktree_content(repo);
        let common = repo.join(".git");
        let mut state = git(&["worktree", "list", "--porcelain"], repo);
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
        ] {
            if common.join(marker).exists() {
                state.push_str(marker);
            }
        }
        Effects {
            refs,
            index,
            worktree,
            state,
        }
    }
}

/// Every file of the working tree outside `.git`, with its content.
fn worktree_content(repo: &Path) -> String {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                let content = std::fs::read(&p).unwrap_or_default();
                out.push(format!(
                    "{} {}",
                    p.strip_prefix(base).unwrap().display(),
                    String::from_utf8_lossy(&content)
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(repo, repo, &mut out);
    out.sort();
    out.join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effects {
    refs: String,
    index: String,
    worktree: String,
    state: String,
}

impl Effects {
    /// Letters of what differs: `r`, `i`, `w`, `s`.
    pub fn letters(&self, after: &Self) -> String {
        [
            ('r', self.refs != after.refs),
            ('i', self.index != after.index),
            ('w', self.worktree != after.worktree),
            ('s', self.state != after.state),
        ]
        .iter()
        .filter(|(_, d)| *d)
        .map(|(c, _)| *c)
        .collect()
    }
}

/// What the executor saw for one case.
#[derive(Debug, Clone)]
pub struct Observation {
    pub code: &'static str,
    pub refs: RefFormat,
    pub hook_ran: bool,
    pub exit: Option<i32>,
    /// Effects left by the denied operation (see [`Effects::letters`]).
    pub residual: String,
    pub moment: Moment,
    /// Hooks that ran, in order.
    pub trace: Vec<String>,
}

/// Run one case on a fresh lab.
pub fn observe(git: &Path, case: &Case) -> Observation {
    let lab = Lab::new(git, case.refs);
    (case.setup)(&lab);
    lab.install_probes(CLIENT_HOOKS);
    let before = lab.effects_state();
    let mut cmd = lab.f.git_command(&lab.f.repo, case.argv);
    cmd.env("PROBE_DENY", case.deny.hook);
    if let Some(a) = case.deny.arg {
        cmd.env("PROBE_DENY_ARG", a);
    }
    if let Some(m) = case.deny.matching {
        cmd.env("PROBE_DENY_MATCH", m);
    }
    for (k, v) in case.env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run case");
    let after = lab.effects_state();
    let hook_ran = lab.denied();
    let residual = before.letters(&after);
    let moment = if !hook_ran {
        Moment::C
    } else if out.status.success() {
        Moment::NotImpeded
    } else if residual.is_empty() {
        Moment::A
    } else {
        Moment::B
    };
    Observation {
        code: case.code,
        refs: case.refs,
        hook_ran,
        exit: out.status.code(),
        residual,
        moment,
        trace: lab.invocations(),
    }
}

/// A row of the reference list: how the case must be published and the moment it must show, for
/// a range of Git versions (inclusive). A version no row covers fails with its own reason.
#[derive(Debug, Clone, Copy)]
pub struct Reference {
    pub code: &'static str,
    pub refs: RefFormat,
    pub from: GitVersion,
    pub to: GitVersion,
    pub published: Published,
    pub moment: Moment,
}

const OLDEST: GitVersion = GitVersion(2, 38, 0);
const ANY: GitVersion = GitVersion(u32::MAX, 0, 0);

/// Published list of ADR-GRD-002 § 1–3 (Enmienda 2026-10-04). Files backend: verified by
/// SPIKE-GRD-001 on macOS with Git 2.38.5, 2.50.1 and 2.56.0, and by this executor with 2.50.1
/// (macOS) and 2.55.0 (Linux and macOS CI runners).
/// Reftable: 2.56.0 (spike), 2.50.1 and 2.55.0 (this executor, 2026-10-05) and 2.56.0 (this
/// executor in the Linux container, 2026-10-06). Windows: Pendiente: etapa de validación
/// multiplataforma.
pub const REFERENCE: &[Reference] = &[
    r("commit", Published::Impedible, Moment::A),
    r("commit-second-line", Published::Impedible, Moment::A),
    r("commit-no-verify", Published::DeclaredSkip, Moment::C),
    r("commit-config-override", Published::DeclaredSkip, Moment::C),
    r("commit-config-env", Published::DeclaredSkip, Moment::C),
    r("push", Published::Impedible, Moment::A),
    r("push-no-verify", Published::DeclaredSkip, Moment::C),
    r("send-pack", Published::DeclaredSkip, Moment::C),
    r("force-push", Published::Impedible, Moment::A),
    r("delete-remote", Published::Impedible, Moment::A),
    r("delete-local", Published::Impedible, Moment::A),
    r("update-ref-delete", Published::Impedible, Moment::A),
    r("rename-base", Published::Impedible, Moment::A),
    r("rename-over-base", Published::NotImpedible("B"), Moment::B),
    r("reset-hard", Published::NotImpedible("B"), Moment::B),
    r("rebase", Published::Impedible, Moment::A),
    r("rebase-no-verify", Published::DeclaredSkip, Moment::C),
    r("pull-rebase", Published::Impedible, Moment::A),
    r("merge", Published::NotImpedible("B"), Moment::B),
    r("merge-ff", Published::NotImpedible("B"), Moment::B),
    r("worktree-add-new-branch", Published::Impedible, Moment::A),
    r(
        "worktree-add-existing",
        Published::NotImpedible("no-reconocible"),
        Moment::A,
    ),
    r("worktree-remove", Published::NotImpedible("C"), Moment::C),
    // Reftable (Git >= 2.45): renaming the base away runs no hook (D10).
    Reference {
        code: "rename-base",
        refs: RefFormat::Reftable,
        from: GitVersion(2, 45, 0),
        to: ANY,
        published: Published::NotImpedible("C"),
        moment: Moment::C,
    },
    // Renaming onto the base: the rename itself (deleting `feat`, rewriting `main`) runs no hook
    // either. The only transaction afterwards moves `HEAD` (`0000… ref:refs/heads/main HEAD`),
    // once `main` is already rewritten and `feat` gone; the probe denies it because its stdin
    // names `refs/heads/main`, so the case reads B. A probe that matched only the ref name would
    // read C, as SPIKE-GRD-001 D11 did with 2.56.0 (Enmienda 2026-10-06 of ADR-GRD-002).
    // Measured with 2.50.1 (macOS), 2.55.0 (Linux and macOS CI) and 2.56.0 (Linux container).
    Reference {
        code: "rename-over-base",
        refs: RefFormat::Reftable,
        from: GitVersion(2, 50, 1),
        to: ANY,
        published: Published::NotImpedible("B"),
        moment: Moment::B,
    },
];

const fn r(code: &'static str, published: Published, moment: Moment) -> Reference {
    Reference {
        code,
        refs: RefFormat::Files,
        from: OLDEST,
        to: ANY,
        published,
        moment,
    }
}

/// The reference row of a case for Git `v`, if any.
pub fn reference(code: &str, refs: RefFormat, v: GitVersion) -> Option<&'static Reference> {
    REFERENCE
        .iter()
        .find(|r| r.code == code && r.refs == refs && r.from <= v && v <= r.to)
}

/// Codes whose hook runs before any effect, but that the policy cannot recognise from the hook's
/// input (ADR-GRD-002 § 3, `no-reconocible`).
const UNRECOGNISABLE: &[&str] = &["worktree-add-existing"];

/// How an observation must be published: A → impedible (unless `no-reconocible`), B or C →
/// not impedible with that reason; a skip case whose hook did not run → declared skip.
pub fn derive(obs: &Observation, skip: bool) -> Published {
    match obs.moment {
        Moment::C if skip => Published::DeclaredSkip,
        Moment::A if UNRECOGNISABLE.contains(&obs.code) => {
            Published::NotImpedible("no-reconocible")
        }
        Moment::A => Published::Impedible,
        Moment::B => Published::NotImpedible("B"),
        Moment::C | Moment::NotImpeded => Published::NotImpedible("C"),
    }
}

/// A difference between Git's behaviour and the reference list.
#[derive(Debug, Clone)]
pub struct Mismatch {
    pub version: GitVersion,
    pub expected: Option<Reference>,
    pub observed: Observation,
    pub derived: Published,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let o = &self.observed;
        match &self.expected {
            Some(e) => write!(
                f,
                "{} ({:?}): published {:?} / moment {:?} expected",
                o.code, o.refs, e.published, e.moment
            )?,
            None => write!(
                f,
                "{} ({:?}): git {} is not in the reference list (measure it and add a row)",
                o.code, o.refs, self.version
            )?,
        }
        write!(
            f,
            "; observed moment {:?} (hook ran: {}, exit {:?}, residual '{}') → {:?}; hooks: {}",
            o.moment,
            o.hook_ran,
            o.exit,
            o.residual,
            self.derived,
            o.trace.join(", ")
        )
    }
}

/// Compare an observation made with Git `v` with its reference row. `None` when they agree.
pub fn compare(obs: &Observation, v: GitVersion) -> Option<Mismatch> {
    let expected = reference(obs.code, obs.refs, v).copied();
    let skip = expected.is_some_and(|e| e.published == Published::DeclaredSkip);
    let derived = derive(obs, skip);
    let agrees = expected.is_some_and(|e| derived == e.published && obs.moment == e.moment);
    (!agrees).then(|| Mismatch {
        version: v,
        expected,
        observed: obs.clone(),
        derived,
    })
}

// --- Catalog ---------------------------------------------------------------------------------

fn nothing(_: &Lab) {}

fn staged_change(lab: &Lab) {
    lab.f.write("a.txt", "alpha changed\n");
    lab.f.git(&["add", "a.txt"]);
}

fn ahead_of_remote(lab: &Lab) {
    lab.commit("d.txt", "delta\n", "three");
}

fn amended_main(lab: &Lab) {
    lab.f.write("b.txt", "beta rewritten\n");
    lab.f.git(&["add", "b.txt"]);
    lab.f
        .git(&["commit", "-q", "--no-verify", "--amend", "-m", "two'"]);
}

fn on_feat(lab: &Lab) {
    lab.f.git(&["checkout", "-q", "feat"]);
}

fn feat_merged_in_main(lab: &Lab) {
    // `feat` gets main's history plus one commit: merging it into main is a fast-forward.
    lab.f.git(&["checkout", "-q", "feat"]);
    lab.f.git(&["reset", "-q", "--hard", "main"]);
    lab.commit("e.txt", "epsilon\n", "ff");
    lab.f.git(&["checkout", "-q", "main"]);
}

fn linked_worktree(lab: &Lab) {
    lab.f.git(&["branch", "wt-branch"]);
    let wt = lab.f.root.join("wt2");
    lab.f
        .git(&["worktree", "add", "-q", wt.to_str().unwrap(), "wt-branch"]);
}

const fn case(
    code: &'static str,
    setup: fn(&Lab),
    argv: &'static [&'static str],
    deny: Deny,
) -> Case {
    Case {
        code,
        setup,
        argv,
        env: &[],
        deny,
        refs: RefFormat::Files,
    }
}

/// Every case of the matrix on the files backend, plus the reftable rows when `reftable`.
pub fn catalog(reftable: bool) -> Vec<Case> {
    let mut out = vec![
        case(
            "commit",
            staged_change,
            &["commit", "-q", "-m", "x"],
            Deny::hook("pre-commit"),
        ),
        case(
            "commit-second-line",
            staged_change,
            &["commit", "-q", "--no-verify", "-m", "x"],
            Deny::prepared("refs/heads/main"),
        ),
        case(
            "commit-no-verify",
            staged_change,
            &["commit", "-q", "--no-verify", "-m", "x"],
            Deny::hook("pre-commit"),
        ),
        case(
            "commit-config-override",
            staged_change,
            &[
                "-c",
                "core.hooksPath=/nonexistent-gitraptor-skip",
                "commit",
                "-q",
                "-m",
                "x",
            ],
            Deny::hook("pre-commit"),
        ),
        Case {
            env: &[
                ("GIT_CONFIG_COUNT", "1"),
                ("GIT_CONFIG_KEY_0", "core.hooksPath"),
                ("GIT_CONFIG_VALUE_0", "/nonexistent-gitraptor-skip"),
            ],
            ..case(
                "commit-config-env",
                staged_change,
                &["commit", "-q", "-m", "x"],
                Deny::hook("pre-commit"),
            )
        },
        case(
            "push",
            ahead_of_remote,
            &["push", "-q", "origin", "main"],
            Deny::hook("pre-push"),
        ),
        case(
            "push-no-verify",
            ahead_of_remote,
            &["push", "-q", "--no-verify", "origin", "main"],
            Deny::hook("pre-push"),
        ),
        case(
            "send-pack",
            amended_main,
            &["send-pack", "--force", "../remote.git", "main"],
            Deny::hook("pre-push"),
        ),
        case(
            "force-push",
            amended_main,
            &["push", "-q", "-f", "origin", "main"],
            Deny::hook("pre-push"),
        ),
        case(
            "delete-remote",
            nothing,
            &["push", "-q", "origin", "--delete", "feat"],
            Deny::hook("pre-push"),
        ),
        case(
            "delete-local",
            nothing,
            &["branch", "-D", "feat"],
            Deny::prepared("refs/heads/feat"),
        ),
        case(
            "update-ref-delete",
            nothing,
            &["update-ref", "-d", "refs/heads/feat"],
            Deny::prepared("refs/heads/feat"),
        ),
        case(
            "rename-base",
            nothing,
            &["branch", "-m", "main", "renamed"],
            Deny::prepared("refs/heads/main"),
        ),
        case(
            "rename-over-base",
            on_feat,
            &["branch", "-M", "feat", "main"],
            Deny::prepared("refs/heads/main"),
        ),
        case(
            "reset-hard",
            nothing,
            &["reset", "-q", "--hard", "HEAD~1"],
            Deny::prepared("refs/heads/main"),
        ),
        case(
            "rebase",
            on_feat,
            &["rebase", "-q", "main"],
            Deny::hook("pre-rebase"),
        ),
        case(
            "rebase-no-verify",
            on_feat,
            &["rebase", "-q", "--no-verify", "main"],
            Deny::hook("pre-rebase"),
        ),
        case(
            "pull-rebase",
            on_feat,
            &["pull", "-q", "--rebase", "origin", "main"],
            Deny::hook("pre-rebase"),
        ),
        case(
            "merge",
            nothing,
            &["merge", "-q", "--no-edit", "--no-ff", "feat"],
            Deny::hook("pre-merge-commit"),
        ),
        case(
            "merge-ff",
            feat_merged_in_main,
            &["merge", "-q", "--ff-only", "feat"],
            Deny::prepared("refs/heads/main"),
        ),
        case(
            "worktree-add-new-branch",
            nothing,
            &["worktree", "add", "-q", "-b", "nb", "../wt-new"],
            Deny::prepared("refs/heads/nb"),
        ),
        case(
            "worktree-add-existing",
            nothing,
            &["worktree", "add", "-q", "../wt-existing", "feat"],
            Deny::prepared("HEAD"),
        ),
        case(
            "worktree-remove",
            linked_worktree,
            &["worktree", "remove", "../wt2"],
            Deny::prepared("HEAD"),
        ),
    ];
    if reftable {
        for code in ["rename-base", "rename-over-base"] {
            let base = *out.iter().find(|c| c.code == code).unwrap();
            out.push(Case {
                refs: RefFormat::Reftable,
                ..base
            });
        }
    }
    out
}

// --- Hook processes per command (ADR-GRD-002 § 5, Validación 13) ------------------------------

/// A command whose hook processes are counted.
#[derive(Clone, Copy)]
pub struct CostCommand {
    pub code: &'static str,
    pub setup: fn(&Lab),
    pub argv: &'static [&'static str],
}

fn three_commits_on_feat(lab: &Lab) {
    lab.f.git(&["checkout", "-q", "feat"]);
    lab.commit("f1.txt", "1\n", "f1");
    lab.commit("f2.txt", "2\n", "f2");
}

fn remote_ahead(lab: &Lab) {
    // Another clone pushes one commit to the remote.
    let other = lab.f.root.join("other-clone");
    lab.f.git_in(
        &lab.f.root,
        &[
            "clone",
            "-q",
            lab.remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("z.txt"), "z\n").unwrap();
    lab.f.git_in(&other, &["add", "z.txt"]);
    lab.f.git_in(&other, &["commit", "-q", "-m", "remote"]);
    lab.f.git_in(&other, &["push", "-q", "origin", "main"]);
}

fn dirty_tree(lab: &Lab) {
    lab.f.write("a.txt", "dirty\n");
}

/// The commands of the cost gate: commit, `switch` (new and existing branch), `rebase` of three
/// commits, `fetch` of one ref and `stash` push plus pop.
pub fn cost_commands() -> Vec<CostCommand> {
    vec![
        CostCommand {
            code: "commit",
            setup: staged_change,
            argv: &["commit", "-q", "-m", "x"],
        },
        CostCommand {
            code: "switch-new",
            setup: nothing,
            argv: &["switch", "-q", "-c", "other"],
        },
        CostCommand {
            code: "switch",
            setup: nothing,
            argv: &["switch", "-q", "feat"],
        },
        CostCommand {
            code: "rebase-3",
            setup: three_commits_on_feat,
            argv: &["rebase", "-q", "main"],
        },
        CostCommand {
            code: "fetch",
            setup: remote_ahead,
            argv: &["fetch", "-q", "origin"],
        },
        CostCommand {
            code: "stash",
            setup: dirty_tree,
            argv: &["stash", "-q"],
        },
        CostCommand {
            code: "stash-pop",
            setup: dirty_tree,
            argv: &["stash", "pop", "-q"],
        },
    ]
}

/// Hook invocations of one command, as `hook[:state]` → count, with probes for `hooks` only.
pub fn count_hooks(git: &Path, cmd: &CostCommand, hooks: &[&str]) -> BTreeMap<String, u32> {
    let lab = Lab::new(git, RefFormat::Files);
    (cmd.setup)(&lab);
    if cmd.code == "stash-pop" {
        lab.f.git(&["stash", "-q"]);
    }
    lab.install_probes(hooks);
    let out = lab
        .f
        .git_command(&lab.f.repo, cmd.argv)
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "{}: {}",
        cmd.code,
        String::from_utf8_lossy(&out.stderr)
    );
    let mut counts = BTreeMap::new();
    for line in lab.invocations() {
        let (hook, arg) = line.split_once(' ').unwrap_or((&line, ""));
        let key = if hook == "reference-transaction" {
            format!("{hook}:{arg}")
        } else {
            hook.to_owned()
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

/// Total processes of a count.
pub fn total(counts: &BTreeMap<String, u32>) -> u32 {
    counts.values().sum()
}

/// A count as text, `hook:state xN` sorted, for reports and the reference table.
pub fn render(counts: &BTreeMap<String, u32>) -> String {
    counts
        .iter()
        .map(|(k, v)| format!("{k}x{v}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reference hook-process counts of one command for a Git version range (inclusive bounds), in
/// [`render`] form.
pub struct CostRow {
    pub from: GitVersion,
    pub to: GitVersion,
    pub command: &'static str,
    pub counts: &'static str,
}

/// Hook-process counts measured with the [`COST_HOOKS`] probes, per Git version range. The
/// minimum set ([`MINIMUM_HOOKS`]) is the same counts restricted to its hooks. Every row says
/// where it was measured. A version outside every range fails the gate with its own reason, so
/// a runner upgrade never changes the gate silently: measure it and add a row.
pub const COST_TABLE: &[CostRow] = &[
    // macOS arm64, Apple Git 2.50.1 (Apple Git-155), 2026-10-05; matches SPIKE-GRD-001 05-hookset.
    cost(
        2,
        50,
        1,
        "commit",
        "commit-msgx1 post-commitx1 pre-commitx1 prepare-commit-msgx1 reference-transaction:abortedx1 reference-transaction:committedx2 reference-transaction:preparedx2",
    ),
    cost(
        2,
        50,
        1,
        "switch-new",
        "post-checkoutx1 reference-transaction:abortedx2 reference-transaction:committedx4 reference-transaction:preparedx4",
    ),
    cost(
        2,
        50,
        1,
        "switch",
        "post-checkoutx1 reference-transaction:abortedx2 reference-transaction:committedx3 reference-transaction:preparedx3",
    ),
    cost(
        2,
        50,
        1,
        "rebase-3",
        "post-checkoutx1 post-commitx3 post-rewritex1 pre-rebasex1 prepare-commit-msgx3 reference-transaction:abortedx9 reference-transaction:committedx22 reference-transaction:preparedx22",
    ),
    cost(
        2,
        50,
        1,
        "fetch",
        "reference-transaction:committedx2 reference-transaction:preparedx2",
    ),
    cost(
        2,
        50,
        1,
        "stash",
        "reference-transaction:abortedx2 reference-transaction:committedx5 reference-transaction:preparedx5",
    ),
    cost(
        2,
        50,
        1,
        "stash-pop",
        "reference-transaction:abortedx1 reference-transaction:committedx2 reference-transaction:preparedx2",
    ),
    // Linux (ubuntu-latest) and macOS (macos-latest, Homebrew) CI runners, Git 2.55.0, 2026-10-05:
    // identical on both. `preparing` already exists in 2.55.
    cost(2, 55, 0, "commit", PREPARING_COMMIT),
    cost(2, 55, 0, "switch-new", PREPARING_SWITCH_NEW),
    cost(2, 55, 0, "switch", PREPARING_SWITCH),
    cost(2, 55, 0, "rebase-3", PREPARING_REBASE_3),
    cost(2, 55, 0, "fetch", PREPARING_FETCH),
    cost(2, 55, 0, "stash", PREPARING_STASH),
    cost(2, 55, 0, "stash-pop", PREPARING_STASH_POP),
    // Linux container (xplat/run-linux.sh, arm64), Git 2.56.0 built from source, 2026-10-06:
    // identical to 2.55.0.
    cost(2, 56, 0, "commit", PREPARING_COMMIT),
    cost(2, 56, 0, "switch-new", PREPARING_SWITCH_NEW),
    cost(2, 56, 0, "switch", PREPARING_SWITCH),
    cost(2, 56, 0, "rebase-3", PREPARING_REBASE_3),
    cost(2, 56, 0, "fetch", PREPARING_FETCH),
    cost(2, 56, 0, "stash", PREPARING_STASH),
    cost(2, 56, 0, "stash-pop", PREPARING_STASH_POP),
    // Linux container (xplat/run-linux.sh, arm64), 2026-10-06: Git 2.38.5 built from source (the
    // minimum, NFR-07) and the distro's 2.43.0, identical. Before the symbolic-ref transactions
    // (`ref:<target>`, SPIKE-GRD-001 § 3) moving `HEAD` runs no `reference-transaction`.
    cost(2, 38, 5, "commit", LEGACY_COMMIT),
    cost(2, 38, 5, "switch-new", LEGACY_SWITCH_NEW),
    cost(2, 38, 5, "switch", LEGACY_SWITCH),
    cost(2, 38, 5, "rebase-3", LEGACY_REBASE_3),
    cost(2, 38, 5, "fetch", LEGACY_FETCH),
    cost(2, 38, 5, "stash", LEGACY_STASH),
    cost(2, 38, 5, "stash-pop", LEGACY_STASH_POP),
    cost(2, 43, 0, "commit", LEGACY_COMMIT),
    cost(2, 43, 0, "switch-new", LEGACY_SWITCH_NEW),
    cost(2, 43, 0, "switch", LEGACY_SWITCH),
    cost(2, 43, 0, "rebase-3", LEGACY_REBASE_3),
    cost(2, 43, 0, "fetch", LEGACY_FETCH),
    cost(2, 43, 0, "stash", LEGACY_STASH),
    cost(2, 43, 0, "stash-pop", LEGACY_STASH_POP),
];

// Counts shared by several measured versions (each version still has its own rows).
const PREPARING_COMMIT: &str = "commit-msgx1 post-commitx1 pre-commitx1 prepare-commit-msgx1 reference-transaction:abortedx1 reference-transaction:committedx2 reference-transaction:preparedx2 reference-transaction:preparingx2";
const PREPARING_SWITCH_NEW: &str = "post-checkoutx1 reference-transaction:abortedx2 reference-transaction:committedx4 reference-transaction:preparedx4 reference-transaction:preparingx4";
const PREPARING_SWITCH: &str = "post-checkoutx1 reference-transaction:abortedx2 reference-transaction:committedx3 reference-transaction:preparedx3 reference-transaction:preparingx3";
const PREPARING_REBASE_3: &str = "post-checkoutx1 post-commitx3 post-rewritex1 pre-rebasex1 prepare-commit-msgx3 reference-transaction:abortedx9 reference-transaction:committedx22 reference-transaction:preparedx22 reference-transaction:preparingx22";
const PREPARING_FETCH: &str = "reference-transaction:committedx2 reference-transaction:preparedx2 reference-transaction:preparingx2";
const PREPARING_STASH: &str = "reference-transaction:abortedx2 reference-transaction:committedx5 reference-transaction:preparedx5 reference-transaction:preparingx5";
const PREPARING_STASH_POP: &str = "reference-transaction:abortedx1 reference-transaction:committedx2 reference-transaction:preparedx2 reference-transaction:preparingx2";
const LEGACY_COMMIT: &str = "commit-msgx1 post-commitx1 pre-commitx1 prepare-commit-msgx1 reference-transaction:committedx1 reference-transaction:preparedx1";
const LEGACY_SWITCH_NEW: &str =
    "post-checkoutx1 reference-transaction:committedx1 reference-transaction:preparedx1";
const LEGACY_SWITCH: &str = "post-checkoutx1";
const LEGACY_REBASE_3: &str = "post-checkoutx1 post-commitx3 post-rewritex1 pre-rebasex1 prepare-commit-msgx3 reference-transaction:abortedx6 reference-transaction:committedx15 reference-transaction:preparedx15";
const LEGACY_FETCH: &str = "reference-transaction:committedx1 reference-transaction:preparedx1";
const LEGACY_STASH: &str = "reference-transaction:committedx3 reference-transaction:preparedx3";
const LEGACY_STASH_POP: &str = "reference-transaction:abortedx1 reference-transaction:committedx1 reference-transaction:preparedx1";

const fn cost(
    major: u32,
    minor: u32,
    patch: u32,
    command: &'static str,
    counts: &'static str,
) -> CostRow {
    CostRow {
        from: GitVersion(major, minor, patch),
        to: GitVersion(major, minor, patch),
        command,
        counts,
    }
}

/// Restrict a count to the hooks of `hooks` (`reference-transaction:<state>` keeps its state).
pub fn restrict(counts: &BTreeMap<String, u32>, hooks: &[&str]) -> BTreeMap<String, u32> {
    counts
        .iter()
        .filter(|(k, _)| hooks.contains(&k.split(':').next().unwrap_or(k)))
        .map(|(k, v)| (k.clone(), *v))
        .collect()
}

/// The rows that cover `v`. Empty: the version is not in the table (its own failure reason).
pub fn cost_rows(table: &'static [CostRow], v: GitVersion) -> Vec<&'static CostRow> {
    table.iter().filter(|r| r.from <= v && v <= r.to).collect()
}
