//! US-GRD-004, cases found in the review of D9 (repair) and of the uninstall of a protection
//! that stopped being active: a repair never leaves files of ours unlisted, never replaces a
//! file that is not ours, works when only the hooks folder was deleted, and the developer can
//! still remove an inactive install. Real `raptor` over the temporary machine of `guard_machine`.
//! Unix only (Pendiente: etapa de validación multiplataforma).
#![cfg(unix)]

mod guard_machine;

use gitraptor_api::guard::ProtectionState;
use guard_machine::{Machine, text};
use serde_json::Value;

#[test]
fn fake_agent_entry() {
    guard_machine::fake_agent_entry();
}

const HOOK: &str = "#!/bin/sh\nexit 0\n";

fn hooks(m: &Machine) -> Value {
    m.status_json(&m.f.repo)["hooks"].clone()
}

fn protected(m: &Machine) {
    m.add(&m.f.repo);
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
}

#[test]
fn a_repair_works_when_only_the_hooks_folder_was_deleted() {
    let m = Machine::new();
    protected(&m);
    std::fs::remove_dir_all(m.common().join("gitraptor/hooks")).unwrap();
    assert_eq!(hooks(&m)["cause"], "folder-missing");
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(hooks(&m)["status"], "active");
    assert_eq!(m.status(&m.f.repo).state, ProtectionState::HooksOnly);
}

#[test]
fn an_inactive_protection_can_still_be_removed() {
    let m = Machine::new();
    protected(&m);
    m.script(&m.common().join("gitraptor/hooks/pre-push"), HOOK);
    assert_eq!(hooks(&m)["cause"], "dispatcher-altered");
    let out = m.uninstall(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!m.common().join("gitraptor/hooks/pre-push").exists());
    assert_eq!(hooks(&m)["status"], "not-installed");
    assert!(
        !m.git(&m.f.repo, &["config", "core.hooksPath"])
            .status
            .success()
    );
}

#[test]
fn removing_a_taken_over_protection_leaves_the_other_tools_key() {
    let m = Machine::new();
    protected(&m);
    m.script(&m.f.repo.join(".husky/_/pre-commit"), HOOK);
    m.git_ok(&m.f.repo, &["config", "core.hooksPath", ".husky/_"]);
    assert_eq!(hooks(&m)["cause"], "hookspath-changed");
    let out = m.uninstall(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!m.common().join("gitraptor").exists());
    assert_eq!(
        m.git_ok(&m.f.repo, &["config", "--local", "core.hooksPath"]),
        ".husky/_"
    );
    assert_eq!(hooks(&m)["status"], "not-installed");
}

#[test]
fn a_repair_that_changes_the_chain_leaves_nothing_unlisted() {
    let m = Machine::new();
    // A hook of the repo that gets a dispatcher of its own (chain-only).
    m.script(&m.common().join("hooks/post-checkout"), HOOK);
    protected(&m);
    assert!(m.common().join("gitraptor/hooks/post-checkout").exists());
    // Another tool takes `core.hooksPath` over, with other hooks.
    m.script(&m.f.repo.join(".husky/_/pre-commit"), HOOK);
    m.git_ok(&m.f.repo, &["config", "core.hooksPath", ".husky/_"]);
    assert_eq!(hooks(&m)["cause"], "hookspath-changed");
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(hooks(&m)["status"], "active");
    // The dispatcher of the old chain is gone: it was ours and untouched.
    assert!(!m.common().join("gitraptor/hooks/post-checkout").exists());
    // Nothing is left behind when the protection is removed afterwards.
    let out = m.uninstall(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!m.common().join("gitraptor").exists());
    assert_eq!(
        m.git_ok(&m.f.repo, &["config", "--local", "core.hooksPath"]),
        ".husky/_"
    );
}

#[test]
fn a_repair_never_replaces_a_file_that_is_not_ours() {
    let m = Machine::new();
    protected(&m);
    // The other tool's key chains a hook the install did not have: it would need a dispatcher
    // named like a file somebody put in the folder.
    m.script(&m.f.repo.join(".husky/_/post-merge"), HOOK);
    m.git_ok(&m.f.repo, &["config", "core.hooksPath", ".husky/_"]);
    let foreign = m.common().join("gitraptor/hooks/post-merge");
    std::fs::write(&foreign, "not ours\n").unwrap();
    let out = m.protect(&m.f.repo);
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(std::fs::read_to_string(&foreign).unwrap(), "not ours\n");
    assert_eq!(hooks(&m)["status"], "inactive");
}
