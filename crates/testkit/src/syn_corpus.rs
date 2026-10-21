//! R-SYN: the synthetic corpus of pairs with a known merge outcome of SPIKE-CKP-001, for the
//! fidelity test of the dry merge against `git merge-tree`.
//!
//! One repository, one case per `<case>/base`, `<case>/ours` and `<case>/theirs` branch (31
//! pairs): content, add/add, modify/delete, renames, a directory rename, directory/file,
//! binary, submodule, LFS pointer, CRLF, `merge` attributes, lines that mimic markers, several
//! hunks, symlink, mode change, two criss-cross histories and the four-session demo of the BRD
//! § 13. Built with the machine's Git in a directory of the caller, with an isolated home and
//! fixed dates; the edits are made here, so the corpus is the same on every OS.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::fixture::{HOME_GITCONFIG, git_from_path};

/// One pair of the corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynCase {
    pub name: String,
    pub repo: PathBuf,
    /// Hex ids of the two tips.
    pub ours: String,
    pub theirs: String,
    /// A pair of the BRD § 13 demo.
    pub demo: bool,
}

/// Builds the corpus under `dir` (created; it must not exist) and returns its 31 pairs.
///
/// # Panics
/// If `dir` exists or a Git command fails: this is test support.
pub fn generate(dir: &Path) -> Vec<SynCase> {
    assert!(!dir.exists(), "{} exists", dir.display());
    let syn = Syn::init(dir);
    let mut cases = Vec::new();
    syn.content_cases(&mut cases);
    syn.tree_cases(&mut cases);
    syn.special_cases(&mut cases);
    syn.criss_cross(&mut cases);
    syn.demo(&mut cases);
    syn.git(&["commit-graph", "write", "--reachable"]);
    cases
}

/// `n` lines `"<prefix> <i>"`, like `seq 1 n | sed "s|^|prefix |"`.
fn lines(n: u32, prefix: &str) -> String {
    (1..=n).map(|i| format!("{prefix} {i}\n")).collect()
}

/// A function body whose line `"    let x = <i>"` the cases edit.
fn fn_file() -> String {
    format!("fn main() {{\n{}}}\n", lines(30, "    let x ="))
}

type Edit<'a> = &'a dyn Fn(&Syn);

struct Syn {
    git: PathBuf,
    home: PathBuf,
    repo: PathBuf,
}

impl Syn {
    fn init(dir: &Path) -> Self {
        let home = dir.join("home");
        let repo = dir.join("repo");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&repo).expect("repo");
        std::fs::write(home.join(".gitconfig"), HOME_GITCONFIG).expect(".gitconfig");
        let syn = Self {
            git: git_from_path(),
            home,
            repo,
        };
        syn.git(&["init", "-q", "-b", "main"]);
        syn.git(&["config", "core.autocrlf", "false"]);
        // Modes are set with `update-index --chmod`, the same on every OS.
        syn.git(&["config", "core.fileMode", "false"]);
        syn.git(&["config", "core.symlinks", "false"]);
        syn.write("README", lines(40, "line").as_bytes());
        syn.git(&["add", "README"]);
        syn.git(&["commit", "-q", "-m", "root"]);
        syn
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.git);
        c.args(args).current_dir(&self.repo);
        if cfg!(unix) {
            c.env_clear().env("PATH", "/usr/bin:/bin");
        } else {
            c.env("USERPROFILE", &self.home);
        }
        c.env("HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", "2026-10-04T10:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-10-04T10:00:00Z");
        c
    }

    fn git(&self, args: &[&str]) -> String {
        let out = self.command(args).output().expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// `git hash-object -w --stdin` of `data`.
    fn blob(&self, data: &[u8]) -> String {
        use std::io::Write;
        let mut child = self
            .command(&["hash-object", "-w", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("run git");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(data)
            .expect("write");
        let out = child.wait_with_output().expect("wait git");
        assert!(out.status.success(), "hash-object failed");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn id(&self, rev: &str) -> String {
        self.git(&["rev-parse", rev]).trim().to_owned()
    }

    fn write(&self, rela: &str, data: &[u8]) {
        let p = self.repo.join(rela);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("dir");
        }
        std::fs::write(p, data).expect("write");
    }

    fn append(&self, rela: &str, data: &[u8]) {
        let mut all = std::fs::read(self.repo.join(rela)).expect("read");
        all.extend_from_slice(data);
        self.write(rela, &all);
    }

    /// Replaces every line equal to `from` (without its `\n`) by `to`, like `sed s/^from$/to/`.
    fn sub(&self, rela: &str, from: &str, to: &str) {
        let text = std::fs::read_to_string(self.repo.join(rela)).expect("read");
        let out: String = text
            .split_inclusive('\n')
            .map(|l| match l.strip_suffix('\n') {
                Some(body) if body == from => format!("{to}\n"),
                None if l == from => to.to_owned(),
                _ => l.to_owned(),
            })
            .collect();
        assert_ne!(out, text, "{rela}: no line {from:?}");
        self.write(rela, out.as_bytes());
    }

    fn delete_line(&self, rela: &str, line: &str) {
        let text = std::fs::read_to_string(self.repo.join(rela)).expect("read");
        let out: String = text
            .split_inclusive('\n')
            .filter(|l| l.strip_suffix('\n') != Some(line))
            .collect();
        self.write(rela, out.as_bytes());
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.commit_index(msg);
    }

    fn commit_index(&self, msg: &str) {
        self.git(&["commit", "-q", "-m", msg, "--allow-empty"]);
    }

    fn case(&self, name: &str, demo: bool) -> SynCase {
        self.case_of(
            name,
            &format!("{name}/ours"),
            &format!("{name}/theirs"),
            demo,
        )
    }

    fn case_of(&self, name: &str, ours: &str, theirs: &str, demo: bool) -> SynCase {
        SynCase {
            name: name.to_owned(),
            repo: self.repo.clone(),
            ours: self.id(ours),
            theirs: self.id(theirs),
            demo,
        }
    }

    /// `<name>/base` from `main`, and `ours` and `theirs` from it; each edit is committed with
    /// `git add -A`, or with the index as it is when `index_only`.
    fn run(&self, name: &str, base: Edit<'_>, ours: Edit<'_>, theirs: Edit<'_>, index_only: bool) {
        let commit = |msg: &str| {
            if index_only {
                self.commit_index(msg);
            } else {
                self.commit(msg);
            }
        };
        let b = format!("{name}/base");
        self.git(&["checkout", "-q", "-b", &b, "main"]);
        base(self);
        commit(&format!("{name} base"));
        for (side, edit) in [("ours", ours), ("theirs", theirs)] {
            // `-f`: an entry staged with `update-index` has no file on disk.
            self.git(&["checkout", "-q", "-f", "-b", &format!("{name}/{side}"), &b]);
            edit(self);
            commit(&format!("{name} {side}"));
        }
        self.git(&["checkout", "-q", "-f", "main"]);
    }

    fn content_cases(&self, cases: &mut Vec<SynCase>) {
        let fn_base = |s: &Syn| s.write("f.rs", fn_file().as_bytes());
        let edit = |n: u32, side: &'static str| {
            move |s: &Syn| {
                s.sub(
                    "f.rs",
                    &format!("    let x = {n}"),
                    &format!("    let x = {n} + {side};"),
                );
            }
        };
        // Same function, same line.
        self.run(
            "same-line",
            &fn_base,
            &edit(10, "ours"),
            &edit(10, "theirs"),
            false,
        );
        cases.push(self.case("same-line", false));
        // Contiguous lines (Git conflicts on adjacent changes).
        self.run(
            "adjacent-lines",
            &fn_base,
            &edit(10, "ours"),
            &edit(11, "theirs"),
            false,
        );
        cases.push(self.case("adjacent-lines", false));
        // Same file, distant lines: overlap without conflict.
        self.run(
            "overlap-clean",
            &fn_base,
            &edit(3, "ours"),
            &edit(25, "theirs"),
            false,
        );
        cases.push(self.case("overlap-clean", false));
        // Add/add with different and with the same content.
        let none = |_: &Syn| {};
        let new = |text: &'static str| move |s: &Syn| s.write("new.txt", text.as_bytes());
        self.run("add-add", &none, &new("ours\n"), &new("theirs\n"), false);
        cases.push(self.case("add-add", false));
        self.run("add-add-same", &none, &new("same\n"), &new("same\n"), false);
        cases.push(self.case("add-add-same", false));
        // Modify/delete.
        self.run(
            "modify-delete",
            &|s| s.write("md.txt", lines(10, "a").as_bytes()),
            &|s| s.append("md.txt", b"more\n"),
            &|s| {
                s.git(&["rm", "-q", "md.txt"]);
            },
            false,
        );
        cases.push(self.case("modify-delete", false));
    }

    fn tree_cases(&self, cases: &mut Vec<SynCase>) {
        let old = |s: &Syn| s.write("old.txt", lines(30, "r").as_bytes());
        let mv = |to: &'static str| {
            move |s: &Syn| {
                s.git(&["mv", "old.txt", to]);
            }
        };
        let theirs_r5 = |s: &Syn| s.sub("old.txt", "r 5", "r 5 theirs");
        // Rename with a modification on the other side (clean).
        self.run("rename-modify", &old, &mv("renamed.txt"), &theirs_r5, false);
        cases.push(self.case("rename-modify", false));
        // Rename/rename to different names.
        self.run(
            "rename-rename",
            &old,
            &mv("ours-name.txt"),
            &mv("theirs-name.txt"),
            false,
        );
        cases.push(self.case("rename-rename", false));
        // Rename with a conflicting modification.
        self.run(
            "rename-modify-conflict",
            &old,
            &|s| {
                s.git(&["mv", "old.txt", "renamed.txt"]);
                s.sub("renamed.txt", "r 5", "r 5 ours");
            },
            &theirs_r5,
            false,
        );
        cases.push(self.case("rename-modify-conflict", false));
        // Directory rename vs a new file in the old directory (`merge.directoryRenames=conflict`).
        self.run(
            "dir-rename",
            &|s| {
                s.write("d/one.txt", lines(20, "one").as_bytes());
                s.write("d/two.txt", lines(20, "two").as_bytes());
            },
            &|s| {
                s.git(&["mv", "d", "e"]);
            },
            &|s| s.write("d/new.txt", lines(20, "new").as_bytes()),
            false,
        );
        cases.push(self.case("dir-rename", false));
        // Directory/file.
        self.run(
            "dir-file",
            &|_| {},
            &|s| s.write("x", b"file\n"),
            &|s| s.write("x/y", b"inside\n"),
            false,
        );
        cases.push(self.case("dir-file", false));
        // A file replaced by a directory filled only with renames (a data-loss case of
        // gix-merge 0.21), without and with a conflict inside the renamed directory.
        let f2_base = |s: &Syn| {
            s.write("a/b", lines(20, "ab").as_bytes());
            s.write("x/1", lines(20, "x1").as_bytes());
            s.write("x/2", lines(20, "x2").as_bytes());
        };
        let f2_ours = |s: &Syn| {
            s.git(&["rm", "-q", "a/b"]);
            std::fs::create_dir_all(s.repo.join("a/b")).expect("dir");
            s.git(&["mv", "x/1", "a/b/1"]);
            s.git(&["mv", "x/2", "a/b/2"]);
        };
        self.run(
            "f2-dir-renames",
            &f2_base,
            &f2_ours,
            &|s| s.sub("a/b", "ab 3", "ab 3 theirs"),
            false,
        );
        cases.push(self.case("f2-dir-renames", false));
        self.run(
            "f2-rename-conflict",
            &f2_base,
            &|s| {
                f2_ours(s);
                s.sub("a/b/1", "x1 5", "x1 5 ours");
            },
            &|s| s.sub("x/1", "x1 5", "x1 5 theirs"),
            false,
        );
        cases.push(self.case("f2-rename-conflict", false));
    }

    fn special_cases(&self, cases: &mut Vec<SynCase>) {
        // Binary modified on both sides.
        let mut bin = vec![1u8; 4096];
        bin.push(0);
        self.run(
            "binary",
            &|s| s.write("bin.dat", &bin),
            &|s| s.append("bin.dat", b"ours\0"),
            &|s| s.append("bin.dat", b"theirs\0"),
            false,
        );
        cases.push(self.case("binary", false));
        // Submodule (gitlink) moved to different commits on each side.
        let gitlink = |rev: &str| format!("160000,{},sub", self.id(rev));
        let (a, o, t) = (
            gitlink("main"),
            gitlink("same-line/ours"),
            gitlink("same-line/theirs"),
        );
        self.run(
            "submodule",
            &|s| {
                s.git(&["update-index", "--add", "--cacheinfo", &a]);
            },
            &|s| {
                s.git(&["update-index", "--cacheinfo", &o]);
            },
            &|s| {
                s.git(&["update-index", "--cacheinfo", &t]);
            },
            true,
        );
        cases.push(self.case("submodule", false));
        // LFS pointers: text in the tree, no LFS filter involved.
        let lfs = |oid: u32, size: u32| {
            move |s: &Syn| {
                let text = format!(
                    "version https://git-lfs.github.com/spec/v1\noid sha256:{oid:064}\nsize {size}\n"
                );
                s.write("big.bin", text.as_bytes());
            }
        };
        self.run(
            "lfs-pointer",
            &|s| {
                lfs(1, 100)(s);
                s.write(
                    ".gitattributes",
                    b"big.bin filter=lfs diff=lfs merge=lfs -text\n",
                );
            },
            &lfs(2, 200),
            &lfs(3, 300),
            false,
        );
        cases.push(self.case("lfs-pointer", false));
        // CRLF file, same line.
        self.run(
            "crlf",
            &|s| s.write("crlf.txt", lines(20, "c").replace('\n', "\r\n").as_bytes()),
            &|s| s.sub("crlf.txt", "c 7\r", "c 7 ours\r"),
            &|s| s.sub("crlf.txt", "c 7\r", "c 7 theirs\r"),
            false,
        );
        cases.push(self.case("crlf", false));
        // `merge=union` and `-merge` in `.gitattributes`: rules the dry merge never applies.
        self.run(
            "union-attr",
            &|s| {
                s.write(".gitattributes", b"u.txt merge=union\n");
                s.write("u.txt", lines(5, "u").as_bytes());
            },
            &|s| s.append("u.txt", b"ours\n"),
            &|s| s.append("u.txt", b"theirs\n"),
            false,
        );
        cases.push(self.case("union-attr", false));
        self.run(
            "nomerge-attr",
            &|s| {
                s.write(".gitattributes", b"n.txt -merge\n");
                s.write("n.txt", lines(30, "n").as_bytes());
            },
            &|s| s.sub("n.txt", "n 2", "n 2 ours"),
            &|s| s.sub("n.txt", "n 28", "n 28 theirs"),
            false,
        );
        cases.push(self.case("nomerge-attr", false));
        // Lines that mimic conflict markers, with a real conflict below them.
        self.run(
            "mimic-markers",
            &|s| {
                let text = format!(
                    "<<<<<<< not a marker\n=======\n>>>>>>> fake\n{}",
                    lines(20, "m")
                );
                s.write("mimic.txt", text.as_bytes());
            },
            &|s| s.sub("mimic.txt", "m 10", "m 10 ours"),
            &|s| s.sub("mimic.txt", "m 10", "m 10 theirs"),
            false,
        );
        cases.push(self.case("mimic-markers", false));
        // Several hunks in one file, with auto-merged changes between them.
        self.run(
            "multi-hunk",
            &|s| s.write("h.txt", lines(60, "h").as_bytes()),
            &|s| {
                s.sub("h.txt", "h 5", "h 5 ours");
                s.sub("h.txt", "h 20", "h 20a\nh 20b ours");
                s.sub("h.txt", "h 40", "h 40 ours");
                s.delete_line("h.txt", "h 50");
            },
            &|s| {
                let text = std::fs::read_to_string(s.repo.join("h.txt")).expect("read");
                s.write("h.txt", format!("inserted at top\n{text}").as_bytes());
                s.sub("h.txt", "h 5", "h 5 theirs");
                s.sub("h.txt", "h 30", "h 30 theirs");
                s.sub("h.txt", "h 40", "h 40 theirs");
            },
            false,
        );
        cases.push(self.case("multi-hunk", false));
        // Symlink changed to different targets: staged as mode 120000 blobs, no link on disk.
        let link = |target: &str| format!("120000,{},link", self.blob(target.as_bytes()));
        let (a, o, t) = (link("target-a"), link("target-ours"), link("target-theirs"));
        self.run(
            "symlink",
            &|s| {
                s.git(&["update-index", "--add", "--cacheinfo", &a]);
            },
            &|s| {
                s.git(&["update-index", "--cacheinfo", &o]);
            },
            &|s| {
                s.git(&["update-index", "--cacheinfo", &t]);
            },
            true,
        );
        cases.push(self.case("symlink", false));
        // Mode change on one side, content on the other (clean).
        self.run(
            "mode-content",
            &|s| s.write("exec.sh", lines(10, "e").as_bytes()),
            &|s| {
                s.git(&["update-index", "--chmod=+x", "exec.sh"]);
            },
            &|s| s.append("exec.sh", b"more\n"),
            false,
        );
        cases.push(self.case("mode-content", false));
    }

    /// Two merge-bases (a virtual base): `cc/a` and `cc/b` merged into each other.
    fn criss_cross(&self, cases: &mut Vec<SynCase>) {
        let step = |branch: &str, from: &str, edit: &dyn Fn(&Syn), merge: Option<&str>| {
            self.git(&["checkout", "-q", "-b", branch, from]);
            if let Some(other) = merge {
                self.git(&["merge", "-q", "--no-edit", other]);
            }
            edit(self);
            self.commit(branch);
        };
        step(
            "cc/base",
            "main",
            &|s| s.write("cc.txt", lines(30, "cc").as_bytes()),
            None,
        );
        step(
            "cc/a",
            "cc/base",
            &|s| s.sub("cc.txt", "cc 5", "cc 5 a"),
            None,
        );
        step(
            "cc/b",
            "cc/base",
            &|s| s.sub("cc.txt", "cc 25", "cc 25 b"),
            None,
        );
        let ours = |s: &Syn| s.sub("cc.txt", "cc 15", "cc 15 ours");
        let theirs = |s: &Syn| s.sub("cc.txt", "cc 15", "cc 15 theirs");
        step("cc/ours", "cc/a", &ours, Some("cc/b"));
        step("cc/theirs", "cc/b", &theirs, Some("cc/a"));
        self.git(&["checkout", "-q", "main"]);
        cases.push(self.case_of("criss-cross", "cc/ours", "cc/theirs", false));
        step(
            "cc2/ours",
            "cc/ours",
            &|s| s.sub("cc.txt", "cc 2", "cc 2 o"),
            None,
        );
        step(
            "cc2/theirs",
            "cc/theirs",
            &|s| s.sub("cc.txt", "cc 28", "cc 28 t"),
            None,
        );
        self.git(&["checkout", "-q", "main"]);
        cases.push(self.case_of("criss-cross-clean", "cc2/ours", "cc2/theirs", false));
    }

    /// The BRD § 13 demo: four sessions; agent-1 and agent-2 touch the same function.
    fn demo(&self, cases: &mut Vec<SynCase>) {
        let agent = |n: u32, edit: &dyn Fn(&Syn)| {
            self.git(&[
                "checkout",
                "-q",
                "-b",
                &format!("demo/agent-{n}"),
                "demo/base",
            ]);
            edit(self);
            self.commit(&format!("agent-{n}"));
        };
        self.git(&["checkout", "-q", "-b", "demo/base", "main"]);
        let cart = format!(
            "pub fn total(items: &[u32]) -> u32 {{\n{}    items.iter().sum()\n}}\n",
            lines(8, "    // step")
        );
        self.write("src/cart.rs", cart.as_bytes());
        self.write("src/README.md", lines(20, "doc").as_bytes());
        self.write("src/config.toml", lines(20, "cfg").as_bytes());
        self.commit("demo base");
        let sum = "    items.iter().sum()";
        agent(1, &|s| {
            s.sub("src/cart.rs", sum, "    items.iter().map(|i| i * 2).sum()");
        });
        agent(2, &|s| {
            s.sub(
                "src/cart.rs",
                sum,
                "    items.iter().filter(|i| **i > 0).sum()",
            );
        });
        agent(3, &|s| s.append("src/README.md", b"extra\n"));
        agent(4, &|s| {
            s.sub("src/config.toml", "cfg 1", "cfg 1 = true");
            s.sub("src/cart.rs", "    // step 1", "    // step 1 (agent-4)");
        });
        self.git(&["checkout", "-q", "main"]);
        for (a, b) in [(1, 2), (1, 3), (1, 4), (2, 3), (2, 4), (3, 4)] {
            cases.push(self.case_of(
                &format!("demo-{a}-{b}"),
                &format!("demo/agent-{a}"),
                &format!("demo/agent-{b}"),
                true,
            ));
        }
    }
}
