//! SEC-09 canary repo: every program a user or a repository can configure points to a script
//! that leaves a marker. No read of the layer may run any of them (ADR-GRP-009, Validación 7).
#![cfg(unix)]

mod common;

use std::path::PathBuf;

use common::{Fixture, assert_unchanged, busy_repo, read_everything, script};
use gitraptor_git::cli::GitCli;
use gitraptor_git::{ChangeKind, ReaderOptions, RefName, RepoReader};

const PROGRAMS: &[&str] = &[
    "filter-clean",
    "filter-smudge",
    "filter-process",
    "textconv",
    "diff-external",
    "gpg",
    "pager",
    "credential",
    "fsmonitor-hook",
    "hook-post-index-change",
    "hook-reference-transaction",
    "hook-post-checkout",
];

struct Canary {
    f: Fixture,
    markers: PathBuf,
}

impl Canary {
    fn marker(&self, name: &str) -> PathBuf {
        self.markers.join(name)
    }

    fn fired(&self) -> Vec<String> {
        std::fs::read_dir(&self.markers)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }
}

/// A repo whose `a.txt` was committed through an uppercasing clean filter (like an LFS pointer,
/// the blob differs from the working file), then armed with marker programs everywhere.
fn canary() -> Canary {
    let f = busy_repo();
    let bin = f.root().join("bin");
    let markers = f.root().join("markers");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&markers).unwrap();

    // Commit a.txt through a real clean filter, before arming anything.
    f.write(".gitattributes", "*.txt filter=evil diff=evil\n");
    f.write("a.txt", "alpha\n");
    f.git(&["config", "filter.evil.clean", "tr a-z A-Z"]);
    f.git(&["add", ".gitattributes", "a.txt"]);
    f.git(&["commit", "-q", "-m", "filtered"]);

    // A signed-looking commit, so `log.showSignature` would call `gpg.program`.
    let tree = f.git(&["rev-parse", "HEAD^{tree}"]);
    let parent = f.git(&["rev-parse", "HEAD"]);
    let raw = format!(
        "tree {}\nparent {}\nauthor Test <test@example.com> 1790000000 +0000\ncommitter Test <test@example.com> 1790000000 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n iQEzBAABCAAdFiEEAAAA\n -----END PGP SIGNATURE-----\n\nsigned\n",
        tree.trim(),
        parent.trim()
    );
    let mut child = f
        .git_command(&f.repo, &["hash-object", "-t", "commit", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(raw.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let signed = String::from_utf8(out.stdout).unwrap();
    f.git(&["update-ref", "refs/heads/main", signed.trim()]);

    // Arm every program.
    let arm = |name: &str, tail: &str| -> String {
        let path = bin.join(name);
        script(
            &path,
            &format!(": > '{}'\n{tail}", markers.join(name).display()),
        );
        path.display().to_string()
    };
    let config = |key: &str, value: &str| {
        f.git(&["config", key, value]);
    };
    config("filter.evil.clean", &arm("filter-clean", "cat"));
    config("filter.evil.smudge", &arm("filter-smudge", "cat"));
    config("filter.evil.process", &arm("filter-process", "exit 1"));
    config("filter.evil.required", "true");
    config("diff.evil.textconv", &arm("textconv", "cat \"$1\""));
    config("diff.external", &arm("diff-external", "exit 0"));
    config("gpg.program", &arm("gpg", "exit 1"));
    config("log.showSignature", "true");
    config("core.pager", &arm("pager", "cat"));
    config("pager.log", &arm("pager", "cat"));
    config("credential.helper", &arm("credential", "exit 0"));
    config("core.fsmonitor", &arm("fsmonitor-hook", "exit 1"));
    config(
        "trace2.eventTarget",
        &markers.join("trace2-event").display().to_string(),
    );
    config(
        "trace2.normalTarget",
        &markers.join("trace2-normal").display().to_string(),
    );
    let hooks = f.repo.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for hook in [
        "post-index-change",
        "reference-transaction",
        "post-checkout",
    ] {
        let marker = markers.join(format!("hook-{hook}"));
        script(&hooks.join(hook), &format!(": > '{}'", marker.display()));
    }

    // Dirty stat, same size: Git would run the clean filter to compare contents.
    f.dirty_stat("a.txt");
    assert!(
        std::fs::read_dir(&markers).unwrap().next().is_none(),
        "setup fired a marker"
    );
    Canary { f, markers }
}

#[test]
fn clean_filter_never_runs_and_file_reported_modified() {
    let c = canary();
    let r = RepoReader::open(&c.f.repo, &ReaderOptions::default()).unwrap();
    let status = r.status().unwrap();
    assert!(
        status
            .unstaged
            .iter()
            .any(|ch| ch.path == "a.txt" && ch.kind == ChangeKind::Modified),
        "a.txt must be reported modified without running the filter: {status:?}"
    );
    drop(r);
    assert!(!c.marker("filter-clean").exists());
    assert!(c.fired().is_empty(), "fired: {:?}", c.fired());
}

#[test]
fn user_programs_never_run() {
    let c = canary();
    let before = c.f.fingerprint();
    read_everything(&c.f, &c.f.repo);
    let invoker = c.f.invoker();
    let cli = GitCli::new(&c.f.git, &invoker, &c.f.repo).unwrap();
    let log = cli.log(&RefName::new("main").unwrap(), 5).unwrap();
    assert_eq!(log[0].subject, "signed");
    assert!(c.fired().is_empty(), "fired: {:?}", c.fired());
    assert_unchanged(&before, &c.f.fingerprint(), "reads of the canary repo");
    for p in PROGRAMS {
        assert!(!c.marker(p).exists(), "{p} ran");
    }
}

/// Control run: the canary is armed. Plain `git status` runs the filter (`process` wins over
/// `clean`, and its failure aborts the required filter) and `git log` with `log.showSignature`
/// runs `gpg.program`.
#[test]
fn canary_is_armed_for_plain_git() {
    let c = canary();
    let _ =
        c.f.git_command(&c.f.repo, &["status", "--porcelain"])
            .output()
            .unwrap();
    assert!(
        c.marker("filter-process").exists(),
        "fired: {:?}",
        c.fired()
    );
    let _ = c.f.git_command(&c.f.repo, &["log", "-1"]).output().unwrap();
    assert!(c.marker("gpg").exists(), "fired: {:?}", c.fired());
}
