//! The temporary machine of the end-to-end suites of the dispatcher template 3 (every pushed ref
//! reaches the forbidden paths, and the in-place upgrade of the dispatchers). It builds on the
//! machine of `guard_machine` (a repo "demo" with a bare remote, a temporary profile, a real
//! daemon) and adds what these suites need: the team settings on `main`, commits to push, a
//! simulated agent that runs plain `git`, an install that stands for template 1 or 2, and the
//! constants of the dispatchers. Temporary repos, home and profile only (NFR-01); nothing waits
//! on a fixed sleep.
//!
//! A suite that uses it declares `mod guard_machine; mod td_grd_001_machine;` and a
//! `#[test] fn fake_agent_entry()` that calls [`fake_agent_entry`].
#![allow(dead_code)]

use std::ffi::OsString;
use std::ops::Deref;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use gitraptor_core::guardrails::install::sha256;
use gitraptor_core::guardrails::journal::{FileHash, Journal};
use gitraptor_core::profile::Profile;
use gitraptor_testkit::fixture::copy_executable;

use crate::guard_machine::{FAKE_AGENT, FAKE_AGENT_ARGV, Machine as Base};

/// A floor with a protected branch and no forbidden path.
pub const NO_PATHS: &str = r#"{"policies":{"protectedBranches":{"patterns":["main"]}}}"#;
/// A floor whose forbidden path governs the agents (the default scope).
pub const AGENT_PATHS: &str = r#"{"policies":{"forbiddenPaths":{"patterns":["secrets/"]}}}"#;
/// A floor whose forbidden path governs everyone.
pub const EVERYONE_PATHS: &str =
    r#"{"policies":{"forbiddenPaths":{"patterns":["secrets/"],"appliesTo":"everyone"}}}"#;

/// The path a commit of these suites touches that the floors above forbid.
pub const FORBIDDEN: &str = "secrets/new.txt";

/// The hooks of a template: 1 has three dispatchers, 2 and 3 have five.
pub fn hooks_of(template: u32) -> &'static [&'static str] {
    if template == 1 {
        &["pre-push", "pre-rebase", "reference-transaction"]
    } else {
        &[
            "pre-push",
            "pre-rebase",
            "reference-transaction",
            "pre-commit",
            "commit-msg",
        ]
    }
}

pub fn text(out: &Output) -> String {
    crate::guard_machine::text(out)
}

/// The entry point of the simulated agent, for the `#[test] fn fake_agent_entry` of the suite.
pub fn fake_agent_entry() {
    crate::guard_machine::fake_agent_entry();
}

pub struct Td {
    base: Base,
    /// Outside the fixture root, so the simulated agent is not part of the fingerprint.
    agents: tempfile::TempDir,
}

impl Deref for Td {
    type Target = Base;

    fn deref(&self) -> &Base {
        &self.base
    }
}

impl Td {
    /// "demo" on `main` with a bare remote; `settings`, when given, is the team configuration
    /// committed on `main` and pushed (the floor). The repo is known to the daemon and, with
    /// `protected`, protected with the current template.
    pub fn new(settings: Option<&str>, protected: bool) -> Self {
        let base = Base::new();
        let repo = base.f.repo.clone();
        if let Some(settings) = settings {
            base.f.write(".gitraptor/settings.json", settings);
            base.git_ok(&repo, &["add", ".gitraptor/settings.json"]);
            base.git_ok(&repo, &["commit", "-q", "-m", "team settings"]);
            base.git_ok(&repo, &["push", "-q", "origin", "main"]);
        }
        base.add(&repo);
        let td = Self {
            base,
            agents: tempfile::tempdir().unwrap(),
        };
        if protected {
            let out = td.protect(&repo);
            assert!(out.status.success(), "{}", text(&out));
        }
        td
    }

    pub fn repo(&self) -> PathBuf {
        self.base.f.repo.clone()
    }

    /// A commit of `rel` on a new branch `branch` cut from `main`; the repo goes back to `main`.
    /// Returns the commit.
    pub fn commit_on(&self, branch: &str, rel: &str) -> String {
        let repo = self.repo();
        self.git_ok(&repo, &["switch", "-q", "-c", branch, "main"]);
        self.f.write(rel, "content\n");
        self.git_ok(&repo, &["add", rel]);
        self.git_ok(&repo, &["commit", "-q", "-m", &format!("add {rel}")]);
        let sha = self.git_ok(&repo, &["rev-parse", "HEAD"]);
        self.git_ok(&repo, &["switch", "-q", "main"]);
        sha
    }

    /// `git push origin <refspec>` by the person.
    pub fn human_push(&self, refspec: &str) -> Output {
        self.git(&self.repo(), &["push", "origin", refspec])
    }

    /// `git push origin <refspec>` by the simulated agent: the daemon resolves the actor from
    /// the ancestry (agent, git, hook), never from a claim.
    pub fn agent_push(&self, refspec: &str) -> Output {
        self.agent_git(&["push", "origin", refspec])
    }

    /// `git <args>` as a child of the simulated agent.
    pub fn agent_git(&self, args: &[&str]) -> Output {
        let agent = self.agents.path().join(FAKE_AGENT);
        if !agent.exists() {
            copy_executable(&std::env::current_exe().unwrap(), &agent);
        }
        let mut argv = vec![self.f.git.to_string_lossy().into_owned()];
        argv.extend(args.iter().map(|a| (*a).to_owned()));
        Command::new(&agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(FAKE_AGENT_ARGV, serde_json::to_string(&argv).unwrap())
            .current_dir(self.repo())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    pub fn folder(&self) -> PathBuf {
        self.common().join("gitraptor")
    }

    /// The `template` constant the dispatchers read.
    pub fn conf_template(&self) -> String {
        let conf = std::fs::read_to_string(self.folder().join("dispatch.conf")).unwrap();
        conf.lines()
            .find_map(|l| l.strip_prefix("template\t"))
            .unwrap_or_default()
            .to_owned()
    }

    /// Edits one constant of the installed dispatchers (a moved or failing `raptor`).
    pub fn set_constant(&self, key: &str, value: &str) {
        let conf = self.folder().join("dispatch.conf");
        let text = std::fs::read_to_string(&conf).unwrap();
        let edited: String = text
            .lines()
            .map(|l| match l.split_once('\t') {
                Some((k, _)) if k == key => format!("{k}\t{value}\n"),
                _ => format!("{l}\n"),
            })
            .collect();
        std::fs::write(conf, edited).unwrap();
    }

    /// Leaves the install as one of an older template, coherent with its journal (so it is
    /// active, not altered): stops the daemon, puts `template\t<N>` in `dispatch.conf`, with 1
    /// removes `pre-commit` and `commit-msg`, and rewrites the journal of the profile with that
    /// template, the files that remain and the new hash of `dispatch.conf`, with no upgrade
    /// pending.
    pub fn downgrade_to(&self, template: u32) {
        assert!((1..=2).contains(&template));
        self.stop();
        let folder = self.folder();
        let conf_path = folder.join("dispatch.conf");
        let conf: String = std::fs::read_to_string(&conf_path)
            .unwrap()
            .lines()
            .map(|l| {
                if l.starts_with("template\t") {
                    format!("template\t{template}\n")
                } else {
                    format!("{l}\n")
                }
            })
            .collect();
        std::fs::write(&conf_path, &conf).unwrap();
        let removed: &[&str] = if template == 1 {
            &["hooks/pre-commit", "hooks/commit-msg"]
        } else {
            &[]
        };
        for path in removed {
            std::fs::remove_file(folder.join(path)).unwrap();
        }

        let (profile, _) = Profile::open(self.dirs()).unwrap();
        let entry = profile
            .repo_by_common_dir(&self.common())
            .unwrap()
            .expect("the repo is known");
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        let text = store.guard_keys().unwrap().journal.expect("an install");
        let mut journal = Journal::from_json(&text).expect("a journal of this build");
        journal.template = template;
        journal.upgrade = None;
        journal
            .files
            .retain(|f| !removed.contains(&f.path.as_str()));
        for file in &mut journal.files {
            if file.path == "dispatch.conf" {
                *file = FileHash {
                    path: file.path.clone(),
                    sha256: sha256(conf.as_bytes()),
                };
            }
        }
        store
            .set_guard_keys(Some(Some(&journal.to_json())), None, None, None)
            .unwrap();
    }

    /// Names in the folder that a killed `replace_files` leaves behind.
    pub fn temporaries(&self) -> Vec<String> {
        let mut found = Vec::new();
        let mut dirs = vec![self.folder()];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    dirs.push(path);
                } else if entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".gitraptor.tmp-")
                {
                    found.push(path.to_string_lossy().into_owned());
                }
            }
        }
        found.sort();
        found
    }

    /// Extra environment for the next commands of the machine (a cut point).
    pub fn with_env(&self, key: &'static str, value: impl Into<OsString>) {
        self.extra_env.borrow_mut().push((key, value.into()));
    }

    pub fn clear_env(&self) {
        self.extra_env.borrow_mut().clear();
    }
}
