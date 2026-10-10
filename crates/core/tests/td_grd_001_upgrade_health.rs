//! The health of an install whose dispatchers are being upgraded in place (ADR-GRD-001 § 8): the
//! journal keeps the confirmed hashes and the pending ones, and every file may hold either, but
//! the constants of the new template are valid only with every dispatcher of the new template.
//! Temporary folders only (NFR-01).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use gitraptor_api::guard::{Diagnostic, HooksLayer, HooksStatus, LossCause};
use gitraptor_core::guardrails::health::check_with;
use gitraptor_core::guardrails::install::sha256;
use gitraptor_core::guardrails::journal::{FileHash, Journal, Prior, Stage, Upgrade};
use gitraptor_git::guard_write::FOLDER;

const MANIFEST: &str = "manifest.json";

fn hash(path: &str, content: &str) -> FileHash {
    FileHash {
        path: path.into(),
        sha256: sha256(content.as_bytes()),
    }
}

/// A folder and a journal of an install of template `from` that started an upgrade to 3.
struct Upgrading {
    _tmp: tempfile::TempDir,
    common: PathBuf,
    journal: Journal,
}

/// The content of `path` in the build of template `t` (`b` tells two builds of the same one).
fn content(path: &str, t: &str) -> String {
    format!("{t}:{path}\n")
}

/// `old_files` hold the confirmed hashes of template T; the upgrade lists `new_files` of 3.
fn upgrading(old_files: &[&str], new_files: &[&str]) -> Upgrading {
    let tmp = tempfile::tempdir().unwrap();
    let common = tmp.path().join(".git");
    let folder = common.join(FOLDER);
    std::fs::create_dir_all(folder.join("hooks")).unwrap();
    let raptor = tmp.path().join("raptor");
    std::fs::write(&raptor, b"bin").unwrap();
    let journal = Journal {
        version: Journal::VERSION,
        stage: Stage::Confirmed,
        at_ms: 0,
        common_dir: common.to_string_lossy().into_owned(),
        hooks_dir: folder.join("hooks").to_string_lossy().into_owned(),
        files: old_files
            .iter()
            .map(|p| hash(p, &content(p, "T")))
            .collect(),
        folder: None,
        config: None,
        raptor: raptor.to_string_lossy().into_owned(),
        template: 2,
        instance: "i".into(),
        prior: Prior {
            value: None,
            level: "none".into(),
            dir: String::new(),
        },
        chained: Vec::new(),
        confirms_base: Some("main".into()),
        protected_bases: vec!["main".into()],
        upgrade: Some(Upgrade {
            template: 3,
            files: new_files
                .iter()
                .map(|p| hash(p, &content(p, "3")))
                .collect(),
        }),
    };
    Upgrading {
        _tmp: tmp,
        common,
        journal,
    }
}

impl Upgrading {
    /// Writes `path` with `text`, executable under `hooks/`.
    fn put(&self, path: &str, text: &str) {
        let p = self.common.join(FOLDER).join(path);
        std::fs::write(&p, text).unwrap();
        let mode = if path.starts_with("hooks/") {
            0o755
        } else {
            0o644
        };
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn set(&self, path: &str, template: &str) {
        self.put(path, &content(path, template));
    }

    fn check(&self) -> gitraptor_core::guardrails::health::Health {
        check_with(
            &self.common,
            Some(&self.journal),
            Ok(Some(self.journal.hooks_dir.clone())),
        )
    }

    fn assert_active_outdated(&self, what: &str) {
        let h = self.check();
        assert_eq!(
            h.hooks,
            HooksLayer::of(HooksStatus::Active),
            "{what}: {h:?}"
        );
        assert!(
            h.diagnostics.contains(&Diagnostic::TemplateOutdated),
            "{what}: {h:?}"
        );
    }

    fn assert_altered(&self, what: &str) {
        let h = self.check();
        assert_eq!(
            h.hooks.cause,
            Some(LossCause::DispatcherAltered),
            "{what}: {h:?}"
        );
    }
}

const OLD: &[&str] = &[
    "hooks/pre-push",
    "hooks/pre-commit",
    "dispatch.conf",
    MANIFEST,
];
const NEW: &[&str] = &[
    "hooks/pre-push",
    "hooks/pre-commit",
    "dispatch.conf",
    MANIFEST,
];

#[test]
fn a_pending_upgrade_is_active_at_every_mix_and_the_new_conf_needs_new_dispatchers() {
    // Every mix of old and new dispatchers with the old conf: active, with the note.
    for push in ["T", "3"] {
        for commit in ["T", "3"] {
            let u = upgrading(OLD, NEW);
            u.set("hooks/pre-push", push);
            u.set("hooks/pre-commit", commit);
            u.set("dispatch.conf", "T");
            u.set(MANIFEST, "T");
            u.assert_active_outdated(&format!("old conf, pre-push {push}, pre-commit {commit}"));
        }
    }
    // The new conf with every dispatcher new: valid (and still pending until it is confirmed).
    let u = upgrading(OLD, NEW);
    for path in OLD {
        u.set(path, "3");
    }
    u.assert_active_outdated("everything new");
    // The new conf with a dispatcher of the old template is the broken combination.
    for old in ["hooks/pre-push", "hooks/pre-commit"] {
        let u = upgrading(OLD, NEW);
        for path in OLD {
            u.set(path, "3");
        }
        u.set(old, "T");
        u.assert_altered(&format!("new conf, {old} old"));
    }
    // Neither the confirmed nor the pending hash of a dispatcher: altered, as ever.
    let u = upgrading(OLD, NEW);
    for path in OLD {
        u.set(path, "T");
    }
    u.put("hooks/pre-push", "somebody else's\n");
    u.assert_altered("a foreign dispatcher");

    // Two builds that started the same upgrade: both hashes of a path are accepted.
    let mut u = upgrading(OLD, NEW);
    if let Some(up) = u.journal.upgrade.as_mut() {
        up.files
            .push(hash("hooks/pre-push", &content("hooks/pre-push", "3b")));
    }
    for path in OLD {
        u.set(path, "T");
    }
    u.set("hooks/pre-push", "3b");
    u.assert_active_outdated("the dispatcher of another build");

    // From template 1: the dispatchers that only the upgrade adds may still be absent with the
    // old conf, and are required once the new conf is in place.
    let one = &["hooks/pre-push", "dispatch.conf", MANIFEST];
    let all = &[
        "hooks/pre-push",
        "hooks/pre-commit",
        "hooks/commit-msg",
        "dispatch.conf",
        MANIFEST,
    ];
    let u = upgrading(one, all);
    u.set("hooks/pre-push", "3");
    u.set("dispatch.conf", "T");
    u.set(MANIFEST, "T");
    u.assert_active_outdated("template 1, only the first dispatcher written");
    u.set("hooks/pre-commit", "3");
    u.assert_active_outdated("template 1, a second dispatcher written");
    u.set("dispatch.conf", "3");
    u.assert_altered("template 1, new conf and commit-msg not written");
    u.set("hooks/commit-msg", "3");
    u.assert_active_outdated("template 1, everything written");
}

#[test]
fn listed_covers_the_files_of_a_pending_upgrade() {
    let mut u = upgrading(
        &["hooks/pre-push", "dispatch.conf", MANIFEST],
        &[
            "hooks/commit-msg",
            "hooks/pre-push",
            "hooks/pre-push",
            "dispatch.conf",
        ],
    );
    // The confirmed ones first, then those only the upgrade adds, each path once.
    assert_eq!(
        u.journal.listed(),
        vec![
            "hooks/pre-push",
            "dispatch.conf",
            MANIFEST,
            "hooks/commit-msg"
        ]
    );
    u.journal.upgrade = None;
    assert_eq!(
        u.journal.listed(),
        vec!["hooks/pre-push", "dispatch.conf", MANIFEST]
    );
}
