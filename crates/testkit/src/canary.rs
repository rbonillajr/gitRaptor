//! SEC-09 canary repo (ADR-GRP-009, Validación 7): every program that a user or a repository
//! can configure points to a script that leaves a marker. Reusable by any crate: the engine
//! suites (US-GRP-002) arm it and observe; Time Machine (INF-TMC-001) extends it with the
//! SEC-TMC-02 cases (`core.worktree` outside the repo, hostile `includeIf`, `commit.gpgSign`,
//! `init.templateDir` with hooks) through [`Canary::arm_program`] and [`Canary::config`].
//!
//! Unix only: the marker programs are `/bin/sh` scripts. The Windows canary is pending (see
//! the INF-GRP-001 Dev Spec).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::fixture::Fixture;

/// Every program armed by [`Canary::arm`].
pub const PROGRAMS: &[&str] = &[
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

/// Every program armed by [`Canary::arm_extended`] on top of [`PROGRAMS`]: a custom merge driver
/// (`merge.<x>.driver`, reached through `merge=<x>` in every place Git reads attributes from).
pub const MERGE_PROGRAMS: &[&str] = &["merge-driver"];

/// An armed fixture. Markers live in `<root>/markers`, scripts in `<root>/bin`.
pub struct Canary {
    pub f: Fixture,
    pub bin: PathBuf,
    pub markers: PathBuf,
}

impl Canary {
    /// Arm `f` (a [`Fixture::busy`] repo). `a.txt` is committed through an uppercasing clean
    /// filter, so the blob differs from the working file (as with an LFS pointer), and its stat
    /// is left dirty: Git would run the filter to compare contents.
    pub fn arm(f: Fixture) -> Self {
        Self::arm_with(f, false)
    }

    /// [`Canary::arm`] plus the merge surface (TS-CKP-001, SEC-09, L-04): `merge.evil.driver` is a
    /// marker program and `merge=evil` is set in the tree (committed `.gitattributes`), the index
    /// (a different staged version), the working tree (a third one), `info/attributes` and
    /// `core.attributesFile`; `refs/replace/*` and `info/grafts` rewrite history. Nothing that
    /// merges may run the driver nor honor the rewritten history. Do all `git` work of the test
    /// (worktrees, branches, commits) before calling it: afterwards every `git` runs the armed
    /// hooks. A merge driver marker is in [`MERGE_PROGRAMS`].
    pub fn arm_extended(f: Fixture) -> Self {
        Self::arm_with(f, true)
    }

    fn arm_with(f: Fixture, merge: bool) -> Self {
        let bin = f.root.join("bin");
        let markers = f.root.join("markers");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&markers).unwrap();
        let c = Self { f, bin, markers };
        let f = &c.f;

        // Commit a.txt through a real clean filter, before arming anything.
        let tree_attrs = if merge {
            "*.txt filter=evil diff=evil\n*.dat merge=evil\n*.cfg merge=evil\n"
        } else {
            "*.txt filter=evil diff=evil\n"
        };
        f.write(".gitattributes", tree_attrs);
        f.write("a.txt", "alpha\n");
        f.git(&["config", "filter.evil.clean", "tr a-z A-Z"]);
        f.git(&["add", ".gitattributes", "a.txt"]);
        f.git(&["commit", "-q", "-m", "filtered"]);
        c.signed_commit_on_main();
        if merge {
            c.rewrite_history_and_attributes(tree_attrs);
        }

        c.config("filter.evil.clean", &c.arm_program("filter-clean", "cat"));
        c.config("filter.evil.smudge", &c.arm_program("filter-smudge", "cat"));
        c.config(
            "filter.evil.process",
            &c.arm_program("filter-process", "exit 1"),
        );
        c.config("filter.evil.required", "true");
        c.config(
            "diff.evil.textconv",
            &c.arm_program("textconv", "cat \"$1\""),
        );
        c.config("diff.external", &c.arm_program("diff-external", "exit 0"));
        c.config("gpg.program", &c.arm_program("gpg", "exit 1"));
        c.config("log.showSignature", "true");
        c.config("core.pager", &c.arm_program("pager", "cat"));
        c.config("pager.log", &c.arm_program("pager", "cat"));
        c.config("credential.helper", &c.arm_program("credential", "exit 0"));
        c.config("core.fsmonitor", &c.arm_program("fsmonitor-hook", "exit 1"));
        if merge {
            c.config(
                "merge.evil.driver",
                &c.arm_program("merge-driver", "exit 1"),
            );
            c.config("merge.evil.name", "evil");
        }
        let event = c.markers.join("trace2-event").display().to_string();
        let normal = c.markers.join("trace2-normal").display().to_string();
        c.config("trace2.eventTarget", &event);
        c.config("trace2.normalTarget", &normal);
        let hooks = f.repo.join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        for hook in [
            "post-index-change",
            "reference-transaction",
            "post-checkout",
        ] {
            let marker = c.markers.join(format!("hook-{hook}"));
            script(&hooks.join(hook), &format!(": > '{}'", marker.display()));
        }

        f.dirty_stat("a.txt");
        assert!(c.fired().is_empty(), "setup fired: {:?}", c.fired());
        c
    }

    /// A marker script named `name` that then runs `tail`; returns its absolute path.
    pub fn arm_program(&self, name: &str, tail: &str) -> String {
        let path = self.bin.join(name);
        script(
            &path,
            &format!(": > '{}'\n{tail}", self.markers.join(name).display()),
        );
        path.display().to_string()
    }

    /// Set a key in the repo config.
    pub fn config(&self, key: &str, value: &str) {
        self.f.git(&["config", key, value]);
    }

    pub fn marker(&self, name: &str) -> PathBuf {
        self.markers.join(name)
    }

    /// Markers present, i.e. programs that ran.
    pub fn fired(&self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(&self.markers)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    /// The merge surface of [`Canary::arm_extended`], written before any program is armed.
    fn rewrite_history_and_attributes(&self, tree_attrs: &str) {
        let f = &self.f;
        // Index: a staged version of `.gitattributes`; working tree: a third one.
        let staged = format!("{tree_attrs}*.bin merge=evil\n");
        f.write(".gitattributes", &staged);
        f.git(&["add", ".gitattributes"]);
        f.write(".gitattributes", &format!("{staged}*.dat -merge\n"));
        // `info/attributes` and `core.attributesFile`.
        let info = f.repo.join(".git/info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("attributes"), "* merge=evil\n").unwrap();
        let global = self.bin.join("attributes");
        std::fs::write(&global, "* merge=evil\n").unwrap();
        f.git(&["config", "core.attributesFile", global.to_str().unwrap()]);
        // `refs/replace/*` (the filtered commit is replaced by its grandparent) and a graft that
        // cuts the signed tip from its history.
        let tip = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
        let filtered = f.git(&["rev-parse", "HEAD~1"]).trim().to_owned();
        let older = f.git(&["rev-parse", "HEAD~2"]).trim().to_owned();
        let root = f
            .git(&["rev-list", "--max-parents=0", "HEAD"])
            .trim()
            .to_owned();
        f.git(&["replace", "-f", &filtered, &older]);
        std::fs::write(info.join("grafts"), format!("{tip} {root}\n")).unwrap();
    }

    /// A commit with a `gpgsig` header on `main`, so `log.showSignature` would call `gpg`.
    fn signed_commit_on_main(&self) {
        let f = &self.f;
        let tree = f.git(&["rev-parse", "HEAD^{tree}"]);
        let parent = f.git(&["rev-parse", "HEAD"]);
        let raw = format!(
            "tree {}\nparent {}\nauthor Test <test@example.com> 1790000000 +0000\ncommitter Test <test@example.com> 1790000000 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n iQEzBAABCAAdFiEEAAAA\n -----END PGP SIGNATURE-----\n\nsigned\n",
            tree.trim(),
            parent.trim()
        );
        let mut child = f
            .git_command(&f.repo, &["hash-object", "-t", "commit", "-w", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(raw.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let signed = String::from_utf8(out.stdout).unwrap();
        f.git(&["update-ref", "refs/heads/main", signed.trim()]);
    }
}

/// Write an executable `/bin/sh` script with mode 0755.
///
/// A child shell writes it, so this process never holds a writable descriptor on it: on Linux
/// one held here while another test thread forks is inherited by that child until its `execve`,
/// and running the script in that window fails with `ETXTBSY` (see
/// [`crate::fixture::copy_executable`]).
pub fn script(path: &Path, body: &str) {
    let status = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            r#"printf '%s\n' '#!/bin/sh' "$2" > "$1" && chmod 0755 "$1""#,
            "sh",
        ])
        .arg(path)
        .arg(body)
        .env_clear()
        .status()
        .expect("run /bin/sh");
    assert!(status.success(), "writing {} failed", path.display());
}
