//! Unit tests of the detector `health` (US-GRD-004, ADR-GRD-005 § 1): each cause on a
//! repo-shaped temporary folder with an install the way the journal recorded it.

use std::path::PathBuf;

use gitraptor_api::guard::{Diagnostic, HooksLayer, HooksStatus, LossCause};
use gitraptor_git::guard_write::FOLDER;

use super::constants::{MANIFEST, TEMPLATE_VERSION};
use super::health::{Health, Key, check_with, fingerprint};
use super::install::sha256;
use super::journal::{FileHash, Journal, Prior, Stage};

const DISPATCHER: &[u8] = b"#!/bin/sh\nexit 0\n";

struct Install {
    _tmp: tempfile::TempDir,
    common: PathBuf,
    journal: Journal,
}

fn install() -> Install {
    let tmp = tempfile::tempdir().unwrap();
    let common = tmp.path().join(".git");
    let folder = common.join(FOLDER);
    std::fs::create_dir_all(folder.join("hooks")).unwrap();
    let raptor = tmp.path().join("raptor");
    std::fs::write(&raptor, b"bin").unwrap();
    let mut files = Vec::new();
    for (path, _executable) in [
        ("hooks/pre-push", true),
        ("dispatch.conf", false),
        (MANIFEST, false),
    ] {
        let p = folder.join(path);
        std::fs::write(&p, DISPATCHER).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if _executable { 0o755 } else { 0o644 };
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        files.push(FileHash {
            path: path.into(),
            sha256: sha256(DISPATCHER),
        });
    }
    let journal = Journal {
        version: Journal::VERSION,
        stage: Stage::Confirmed,
        at_ms: 0,
        common_dir: common.to_string_lossy().into_owned(),
        hooks_dir: folder.join("hooks").to_string_lossy().into_owned(),
        files,
        folder: None,
        config: None,
        raptor: raptor.to_string_lossy().into_owned(),
        template: TEMPLATE_VERSION,
        instance: "i".into(),
        prior: Prior {
            value: None,
            level: "none".into(),
            dir: String::new(),
        },
        chained: Vec::new(),
        confirms_base: Some("main".into()),
        protected_bases: vec!["main".into()],
    };
    Install {
        _tmp: tmp,
        common,
        journal,
    }
}

impl Install {
    fn key(&self) -> Key {
        Ok(Some(self.journal.hooks_dir.clone()))
    }

    fn check(&self) -> Health {
        check_with(&self.common, Some(&self.journal), self.key())
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.common.join(FOLDER).join(rel)
    }
}

fn cause(h: &Health) -> Option<LossCause> {
    h.hooks.cause
}

#[test]
fn an_intact_install_is_active() {
    let i = install();
    let h = i.check();
    assert_eq!(h.hooks, HooksLayer::of(HooksStatus::Active));
    assert!(h.diagnostics.is_empty(), "{h:?}");
}

#[test]
fn the_detector_tells_each_cause() {
    let i = install();
    let h = check_with(&i.common, Some(&i.journal), Ok(Some(".husky/_".into())));
    assert_eq!(cause(&h), Some(LossCause::HookspathChanged));
    let h = check_with(&i.common, Some(&i.journal), Ok(None));
    assert_eq!(cause(&h), Some(LossCause::HookspathChanged));

    std::fs::remove_file(i.path("hooks/pre-push")).unwrap();
    assert_eq!(cause(&i.check()), Some(LossCause::DispatcherMissing));
    std::fs::write(i.path("hooks/pre-push"), b"#!/bin/sh\nexit 1\n").unwrap();
    assert_eq!(cause(&i.check()), Some(LossCause::DispatcherAltered));
    std::fs::remove_dir_all(i.path("hooks")).unwrap();
    assert_eq!(cause(&i.check()), Some(LossCause::FolderMissing));
}

#[test]
fn a_missing_raptor_is_a_loss() {
    let i = install();
    std::fs::remove_file(&i.journal.raptor).unwrap();
    assert_eq!(cause(&i.check()), Some(LossCause::BinaryMissing));
}

#[cfg(unix)]
#[test]
fn a_dispatcher_without_the_execute_bit_is_a_loss() {
    use std::os::unix::fs::PermissionsExt;
    let i = install();
    std::fs::set_permissions(
        i.path("hooks/pre-push"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(cause(&i.check()), Some(LossCause::DispatcherNotExecutable));
}

#[test]
fn the_manifest_is_not_the_reference() {
    let i = install();
    std::fs::write(i.path(MANIFEST), b"edited").unwrap();
    assert_eq!(i.check().hooks, HooksLayer::of(HooksStatus::Active));
    // Editing a dispatcher is found even when the manifest is edited to match.
    std::fs::write(i.path("hooks/pre-push"), b"#!/bin/sh\nexit 1\n").unwrap();
    std::fs::write(i.path(MANIFEST), sha256(b"#!/bin/sh\nexit 1\n")).unwrap();
    assert_eq!(cause(&i.check()), Some(LossCause::DispatcherAltered));
}

#[test]
fn an_unreadable_configuration_is_not_a_loss() {
    let i = install();
    let h = check_with(&i.common, Some(&i.journal), Err(()));
    assert_eq!(h.hooks.status, HooksStatus::Active);
    assert!(h.diagnostics.contains(&Diagnostic::ConfigUnreadable));
    // What it can still read is a loss.
    std::fs::remove_file(i.path("hooks/pre-push")).unwrap();
    let h = check_with(&i.common, Some(&i.journal), Err(()));
    assert_eq!(cause(&h), Some(LossCause::DispatcherMissing));
}

#[test]
fn an_outdated_template_is_a_warning_not_a_loss() {
    let mut i = install();
    i.journal.template = TEMPLATE_VERSION - 1;
    let h = i.check();
    assert_eq!(h.hooks.status, HooksStatus::Active);
    assert!(h.diagnostics.contains(&Diagnostic::TemplateOutdated));
}

#[test]
fn a_repo_that_moved_names_the_old_folder() {
    let mut i = install();
    i.journal.hooks_dir = "/somewhere/else/.git/gitraptor/hooks".into();
    let h = check_with(&i.common, Some(&i.journal), i.key());
    assert_eq!(cause(&h), Some(LossCause::RepoMoved));
}

#[test]
fn without_a_journal_the_key_and_the_manifest_make_an_orphan() {
    let i = install();
    let h = check_with(&i.common, None, i.key());
    assert_eq!(h.hooks.status, HooksStatus::Orphaned);
    std::fs::remove_file(i.path(MANIFEST)).unwrap();
    let h = check_with(&i.common, None, i.key());
    assert_eq!(h.hooks.status, HooksStatus::NotInstalled);
    let h = check_with(&i.common, None, Ok(None));
    assert_eq!(h.hooks.status, HooksStatus::NotInstalled);
}

#[test]
fn the_fingerprint_moves_with_what_the_check_reads() {
    let i = install();
    let before = fingerprint(&i.common, &i.journal);
    assert_eq!(before, fingerprint(&i.common, &i.journal));
    std::fs::write(i.path("hooks/pre-push"), b"#!/bin/sh\nexit 1\n").unwrap();
    assert_ne!(before, fingerprint(&i.common, &i.journal));
}
