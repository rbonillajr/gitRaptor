//! `raptor guard install` and `raptor guard uninstall` over an install of an older `raptor`
//! (dogfooding, 2026-10-17): a template 2 install written on 2026-10-07, whose journal recorded
//! the identity of `gitraptor/` before a reboot renumbered the device of the volume (macOS
//! assigns `st_dev` when it mounts a volume; the inode stays). The upgrade failed with an
//! internal error, and the uninstall took the key away and then stopped, leaving the folder
//! without a way out from `raptor`.
//!
//! Real `raptor`, real `raptor-hook` and plain Git over the temporary machine of
//! `td_grd_001_machine`: temporary repos, home and profile only (NFR-01). Unix only; Windows has
//! no channel transport yet (Pendiente: etapa de validación multiplataforma).
#![cfg(unix)]

mod guard_machine;
mod td_grd_001_machine;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use gitraptor_api::guard::{Diagnostic, HooksStatus, ProtectionState};
use gitraptor_core::guardrails::install::sha256;
use gitraptor_core::profile::Profile;
use td_grd_001_machine::{NO_PATHS, Td, hooks_of, text};

#[test]
fn fake_agent_entry() {
    td_grd_001_machine::fake_agent_entry();
}

/// What the uninstall says when its cleanup stopped after the key went back.
const FINISH_WITH_UNINSTALL: &str = "run `raptor guard uninstall` again";

/// Edits the journal of "demo" as JSON (the daemon is stopped first).
fn edit_journal(m: &Td, edit: impl FnOnce(&mut serde_json::Value)) {
    m.stop();
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    let entry = profile
        .repo_by_common_dir(&m.common())
        .unwrap()
        .expect("the repo is known");
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    let text = store.guard_keys().unwrap().journal.expect("an install");
    let mut journal: serde_json::Value = serde_json::from_str(&text).unwrap();
    edit(&mut journal);
    store
        .set_guard_keys(Some(Some(&journal.to_string())), None, None, None)
        .unwrap();
}

/// Forgets the install of "demo" in the profile (a profile lost or reset): the repo keeps
/// whatever it has.
fn forget_install(m: &Td) {
    m.stop();
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    let entry = profile.repo_by_common_dir(&m.common()).unwrap().unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    store.set_guard_keys(Some(None), None, None, None).unwrap();
}

/// The journal of "demo" as JSON, if any.
fn journal(m: &Td) -> Option<serde_json::Value> {
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    let entry = profile.repo_by_common_dir(&m.common()).unwrap().unwrap();
    let (store, _) = profile.open_store(&entry.repo_id).unwrap();
    store
        .guard_keys()
        .unwrap()
        .journal
        .map(|t| serde_json::from_str(&t).unwrap())
}

/// The device of the folder as a reboot that renumbered the volume leaves it in the journal.
fn renumber_device(journal: &mut serde_json::Value) {
    let dev = journal["folder"]["dev"]
        .as_u64()
        .expect("a recorded folder");
    journal["folder"]["dev"] = (dev + 4).into();
}

/// Turns the template 2 install of "demo" into one written by the `raptor` of 2026-10-07: its
/// manifest has neither `prior_hooks_dir` nor `chained`, its `prior` constant is empty, its
/// journal records no `prior.dir` and no `chained`, and the volume was renumbered since.
fn as_of_october_7(m: &Td) {
    m.downgrade_to(2);
    let folder = m.folder();
    let manifest_path = folder.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let object = manifest.as_object_mut().unwrap();
    object.remove("prior_hooks_dir");
    object.remove("chained");
    let manifest = serde_json::to_string_pretty(&manifest).unwrap();
    std::fs::write(&manifest_path, &manifest).unwrap();
    m.set_constant("prior", "");
    let conf = std::fs::read(folder.join("dispatch.conf")).unwrap();
    assert!(
        !String::from_utf8_lossy(&conf).contains("git_sh\t"),
        "no `sh` line, as on 2026-10-07"
    );
    edit_journal(m, |j| {
        for file in j["files"].as_array_mut().unwrap() {
            match file["path"].as_str().unwrap() {
                "manifest.json" => file["sha256"] = sha256(manifest.as_bytes()).into(),
                "dispatch.conf" => file["sha256"] = sha256(&conf).into(),
                _ => {}
            }
        }
        j["prior"].as_object_mut().unwrap().remove("dir");
        j.as_object_mut().unwrap().remove("chained");
        renumber_device(j);
    });
}

fn hooks_path(m: &Td) -> Option<String> {
    let out = m.git(&m.repo(), &["config", "--local", "core.hooksPath"]);
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn assert_protected(m: &Td, why: &str) {
    let status = m.status(&m.repo());
    assert_eq!(status.state, ProtectionState::HooksOnly, "{why}");
    m.git_ok(
        &m.repo(),
        &["switch", "-q", "-c", &format!("side-{}", why.len())],
    );
    let out = m.git(&m.repo(), &["branch", "-D", "main"]);
    assert!(!out.status.success(), "{why}: {}", text(&out));
}

fn assert_removed(m: &Td, why: &str) {
    assert!(!m.folder().exists(), "{why}: the folder is still there");
    assert_eq!(hooks_path(m), None, "{why}");
    let status = m.status(&m.repo());
    assert_eq!(status.state, ProtectionState::Unprotected, "{why}");
    assert_eq!(
        status.hooks.map(|h| h.status),
        Some(HooksStatus::NotInstalled),
        "{why}"
    );
}

/// Replaces `gitraptor/` by a copy of itself (a backup restored): another inode, same files.
fn restore_a_copy(m: &Td) {
    let folder = m.folder();
    let copy = m.common().join("gitraptor-copy");
    let out = std::process::Command::new("/bin/cp")
        .args(["-Rp", folder.to_str().unwrap(), copy.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    std::fs::remove_dir_all(&folder).unwrap();
    std::fs::rename(&copy, &folder).unwrap();
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// (a) The install of 2026-10-07 is upgraded in place to template 3 by `raptor guard install`.
mod legacy_install {
    use super::*;

    #[test]
    fn repo_intact_an_install_of_october_7_is_upgraded_after_a_reboot() {
        let m = Td::new(Some(NO_PATHS), true);
        as_of_october_7(&m);
        assert_protected(&m, "before the upgrade");

        let out = m.protect(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert!(!text(&out).contains("internal error"), "{}", text(&out));
        assert_eq!(m.conf_template(), "3");
        for hook in hooks_of(3) {
            assert!(m.folder().join("hooks").join(hook).is_file(), "{hook}");
        }
        assert_protected(&m, "after the upgrade");
        // The journal follows the folder as it is now.
        let now = std::fs::metadata(m.folder()).unwrap();
        use std::os::unix::fs::MetadataExt;
        let j = journal(&m).unwrap();
        assert_eq!(j["folder"]["dev"].as_u64(), Some(now.dev()));
        assert_eq!(j["template"].as_u64(), Some(3));
    }

    // A folder restored from a copy (another inode) whose files are still the ones the journal
    // recorded is the same install: upgraded, not refused. An edited manifest changes nothing
    // (ADR-GRD-001 Validación 9).
    #[test]
    fn repo_intact_a_restored_copy_of_the_folder_is_upgraded() {
        let m = Td::new(Some(NO_PATHS), true);
        m.downgrade_to(2);
        let folder = m.folder();
        restore_a_copy(&m);
        std::fs::write(folder.join("manifest.json"), "{}").unwrap();

        let out = m.protect(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(m.conf_template(), "3");
        assert_protected(&m, "after the upgrade");
    }

    // A folder whose inode changed and whose dispatchers are not the recorded ones is not
    // adopted silently: it is an altered protection, and installing again goes through the
    // repair screen that says what changes (US-GRD-004), never through the upgrade.
    #[test]
    fn repo_intact_an_altered_copy_is_only_repaired_through_its_screen() {
        let m = Td::new(Some(NO_PATHS), true);
        m.downgrade_to(2);
        let folder = m.folder();
        restore_a_copy(&m);
        std::fs::write(folder.join("hooks/pre-rebase"), "#!/bin/sh\nexit 0\n").unwrap();

        let out = m.protect(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("Repair the protection"),
            "{}",
            text(&out)
        );
        assert_eq!(m.conf_template(), "3");
        assert_protected(&m, "after the repair");
    }
}

/// (b) An uninstall that fails half way never leaves the repo without its key and with a folder
/// nothing can remove: either the protection stays whole, or the next `raptor guard uninstall`
/// finishes it.
mod interrupted_uninstall {
    use super::*;

    // The uninstall of the install of 2026-10-07, whose folder the journal knows under the old
    // device number: removed whole.
    #[test]
    fn repo_intact_the_install_of_october_7_is_removed_whole() {
        let m = Td::new(Some(NO_PATHS), true);
        as_of_october_7(&m);
        let out = m.uninstall(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_removed(&m, "after the uninstall");
        assert_eq!(journal(&m), None);
    }

    // The files cannot be removed (their folder is read-only) after the key went back: the
    // command says the cleanup is pending and how to finish it; the status says a folder is
    // left; the next uninstall finishes it once the files can go, and then installing works.
    #[test]
    fn repo_intact_a_cleanup_that_fails_after_the_key_is_finished_by_the_next_uninstall() {
        let m = Td::new(Some(NO_PATHS), true);
        let hooks = m.folder().join("hooks");
        set_mode(&hooks, 0o555);
        let out = m.uninstall(&m.repo());
        set_mode(&hooks, 0o755);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(!text(&out).contains("internal error"), "{}", text(&out));
        assert!(text(&out).contains(FINISH_WITH_UNINSTALL), "{}", text(&out));
        assert_eq!(hooks_path(&m), None, "the key went back");
        let status = m.status(&m.repo());
        assert_eq!(status.state, ProtectionState::Unprotected);
        assert_eq!(
            status.hooks.map(|h| h.status),
            Some(HooksStatus::NotInstalled)
        );
        assert!(
            status.diagnostics.contains(&Diagnostic::LeftoverFolder),
            "the status says a folder is left: {:?}",
            status.diagnostics
        );
        // Installing says how to get out, instead of only refusing.
        let out = m.protect(&m.repo());
        assert!(!out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("raptor guard uninstall"),
            "{}",
            text(&out)
        );

        let out = m.uninstall(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_removed(&m, "after the second uninstall");
        let out = m.protect(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_protected(&m, "installed again");
    }

    // The same failure, then a restart of the daemon while the files still cannot go: the
    // recovery does not claim it finished, and the next uninstall still can.
    #[test]
    fn repo_intact_a_cleanup_still_failing_at_startup_stays_pending() {
        let m = Td::new(Some(NO_PATHS), true);
        let hooks = m.folder().join("hooks");
        set_mode(&hooks, 0o555);
        let out = m.uninstall(&m.repo());
        assert!(!out.status.success(), "{}", text(&out));
        m.stop();
        let out = m.raptor(&["daemon", "status"]);
        assert!(out.status.success(), "{}", text(&out));
        set_mode(&hooks, 0o755);
        assert!(m.folder().exists());
        let log = std::fs::read_to_string(m.dirs().state.join("daemon.log")).unwrap();
        assert!(log.contains("guard_uninstall_cleanup_pending"), "{log}");
        assert!(!log.contains("guard_uninstall_recovered"), "{log}");
        assert!(
            m.status(&m.repo())
                .diagnostics
                .contains(&Diagnostic::LeftoverFolder)
        );
        let out = m.uninstall(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_removed(&m, "after the second uninstall");
    }

    // A failure before the key (the repo's `config` is not a regular file, M-03) keeps the
    // protection whole and says so; once it is, the uninstall goes through.
    #[test]
    fn repo_intact_a_failure_before_the_key_keeps_the_protection() {
        let m = Td::new(Some(NO_PATHS), true);
        let config = m.common().join("config");
        let real = m.common().join("config.real");
        std::fs::rename(&config, &real).unwrap();
        std::os::unix::fs::symlink(&real, &config).unwrap();

        let out = m.uninstall(&m.repo());
        assert!(!out.status.success(), "{}", text(&out));
        assert!(!text(&out).contains("internal error"), "{}", text(&out));
        assert!(
            text(&out).contains("the protection was not removed"),
            "{}",
            text(&out)
        );
        assert!(hooks_path(&m).is_some(), "the key stays");
        assert!(m.folder().join("hooks/pre-push").is_file());
        assert_protected(&m, "after the refusal");
        assert_eq!(
            journal(&m).unwrap()["stage"].as_str(),
            Some("confirmed"),
            "the protection is recorded whole"
        );

        std::fs::remove_file(&config).unwrap();
        std::fs::rename(&real, &config).unwrap();
        let out = m.uninstall(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_removed(&m, "after the uninstall");
    }

    // A dispatcher someone edited is not GitRaptor's any more: the uninstall removes everything
    // else, leaves it (NFR-01) and says a folder is left.
    #[test]
    fn repo_intact_an_edited_dispatcher_stays_after_the_uninstall() {
        let m = Td::new(Some(NO_PATHS), true);
        let edited = m.folder().join("hooks/pre-rebase");
        std::fs::write(&edited, "#!/bin/sh\nexit 0\n").unwrap();
        let out = m.uninstall(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(hooks_path(&m), None);
        assert_eq!(
            std::fs::read_to_string(&edited).unwrap(),
            "#!/bin/sh\nexit 0\n"
        );
        assert!(!m.folder().join("hooks/pre-push").exists());
        assert!(!m.folder().join("dispatch.conf").exists());
        assert!(
            m.status(&m.repo())
                .diagnostics
                .contains(&Diagnostic::LeftoverFolder)
        );
    }
}

/// (c) A `gitraptor/` folder this profile does not know: `raptor guard uninstall` removes it
/// when it is recognizably ours, with the same announcement and window, and never removes a
/// file it did not write.
mod orphan_folder {
    use super::*;

    // The profile lost the record and the key is still ours: the key goes (never set to a value
    // read from the repo) and the folder with it.
    #[test]
    fn repo_intact_an_orphan_with_the_key_is_removed() {
        let m = Td::new(Some(NO_PATHS), true);
        forget_install(&m);
        assert_eq!(
            m.status(&m.repo()).hooks.map(|h| h.status),
            Some(HooksStatus::Orphaned)
        );
        let out = m.uninstall(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_removed(&m, "after removing the orphan");
        let out = m.protect(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_protected(&m, "installed again");
    }

    // Rene's state on 2026-10-17: the key already gone, the folder left, the profile does not
    // know it. A file someone else put in the folder stays, and the folder with it.
    #[test]
    fn repo_intact_an_orphan_without_the_key_is_removed_but_a_foreign_file_stays() {
        let m = Td::new(Some(NO_PATHS), true);
        forget_install(&m);
        m.git_ok(&m.repo(), &["config", "--unset", "core.hooksPath"]);
        std::fs::write(m.folder().join("notes.txt"), "mine\n").unwrap();
        let out = m.uninstall(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(hooks_path(&m), None);
        assert_eq!(
            std::fs::read_to_string(m.folder().join("notes.txt")).unwrap(),
            "mine\n"
        );
        assert!(!m.folder().join("manifest.json").exists());
        assert!(!m.folder().join("hooks").exists());
    }

    // The repo had its own local `core.hooksPath` (husky) before the install: removing the
    // orphan writes it back, and says so before the window, read from the manifest
    // (ADR-GRD-005 § 1, retirar).
    #[test]
    fn repo_intact_an_orphan_puts_back_the_prior_local_key_and_says_so() {
        let m = Td::new(Some(NO_PATHS), false);
        m.script(&m.repo().join(".husky/_/pre-commit"), "#!/bin/sh\nexit 0\n");
        m.git_ok(&m.repo(), &["config", "core.hooksPath", ".husky/_"]);
        let out = m.protect(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        forget_install(&m);
        let out = m.uninstall(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("core.hooksPath was «.husky/_» before"),
            "{}",
            text(&out)
        );
        assert_eq!(hooks_path(&m).as_deref(), Some(".husky/_"));
        assert!(!m.folder().exists());
    }

    // What the announcement shows is what is applied: a manifest rewritten during the window
    // (another value to write back) refuses the removal, and nothing is touched.
    #[test]
    fn repo_intact_a_manifest_rewritten_during_the_window_is_refused() {
        let m = Td::new(Some(NO_PATHS), false);
        m.script(&m.repo().join(".husky/_/pre-commit"), "#!/bin/sh\nexit 0\n");
        m.git_ok(&m.repo(), &["config", "core.hooksPath", ".husky/_"]);
        let out = m.protect(&m.repo());
        assert!(out.status.success(), "{}", text(&out));
        forget_install(&m);
        // The daemon starts again with a window long enough to rewrite the manifest in it.
        m.with_env("GITRAPTOR_TEST_GUARD_WINDOW_MS", "3000");
        let mut run =
            m.developer_spawn(&["guard", "uninstall", "--yes", m.repo().to_str().unwrap()]);
        run.wait_for("Announced");
        let manifest_path = m.folder().join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["prior_hooks_path"]["value"] = "planted/hooks".into();
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        let out = run.finish();
        assert!(!out.status.success(), "{}", text(&out));
        assert!(!text(&out).contains("internal error"), "{}", text(&out));
        assert_eq!(
            hooks_path(&m),
            Some(m.folder().join("hooks").to_string_lossy().into_owned()),
            "the key stays"
        );
        assert!(m.folder().join("hooks/pre-push").is_file());
    }

    // A folder that is not recognizably ours is never touched.
    #[test]
    fn repo_intact_a_folder_without_our_manifest_is_left_alone() {
        let m = Td::new(Some(NO_PATHS), true);
        forget_install(&m);
        m.git_ok(&m.repo(), &["config", "--unset", "core.hooksPath"]);
        std::fs::write(
            m.folder().join("manifest.json"),
            "{\"note\":\"someone else\"}",
        )
        .unwrap();
        let out = m.uninstall(&m.repo());
        assert!(!out.status.success(), "{}", text(&out));
        assert!(m.folder().join("hooks/pre-push").is_file());
        assert!(m.folder().join("manifest.json").is_file());
    }

    // An agent cannot remove an orphan either: it is the same reserved command.
    #[test]
    fn an_agent_cannot_remove_an_orphan() {
        let m = Td::new(Some(NO_PATHS), true);
        forget_install(&m);
        let out = m.as_agent(&["guard", "uninstall", "--yes", m.repo().to_str().unwrap()]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(m.folder().join("hooks/pre-push").is_file());
        assert!(hooks_path(&m).is_some());
    }
}
