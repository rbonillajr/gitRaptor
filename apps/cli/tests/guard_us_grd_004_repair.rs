//! US-GRD-004, D9: installing again over a protection that stopped being active is a repair,
//! shown as such before the developer allows it, and an interruption in the middle leaves the
//! repo as it was or repaired, never worse (NFR-01, NFR-12). Real `raptor` over the temporary
//! machine of `guard_machine`. Unix only (Pendiente: etapa de validación multiplataforma).
#![cfg(unix)]

mod guard_machine;

use gitraptor_api::guard::{Permission, ProtectionState};
use guard_machine::{Machine, text};
use serde_json::Value;

#[test]
fn fake_agent_entry() {
    guard_machine::fake_agent_entry();
}

const HUSKY_HOOK: &str = "#!/bin/sh\nexit 0\n";

/// A protected "demo" whose `core.hooksPath` another tool then pointed at `.husky/_`.
fn taken_over() -> Machine {
    let m = Machine::new();
    m.add(&m.f.repo);
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    m.script(&m.f.repo.join(".husky/_/pre-commit"), HUSKY_HOOK);
    m.git_ok(&m.f.repo, &["config", "core.hooksPath", ".husky/_"]);
    m
}

fn hooks(m: &Machine) -> Value {
    m.status_json(&m.f.repo)["hooks"].clone()
}

#[test]
fn the_permission_screen_says_it_is_a_repair_and_what_changes() {
    let m = taken_over();
    assert_eq!(hooks(&m)["status"], "inactive");
    let out = m.developer_answering(
        &["guard", "install", m.f.repo.to_str().unwrap()],
        "[y/N]",
        "n\n",
    );
    let shown = text(&out);
    assert!(shown.contains("Repair the protection of"), "{shown}");
    assert!(
        shown.contains("another tool changed `core.hooksPath`"),
        "{shown}"
    );
    assert!(shown.contains(".husky/_"), "{shown}");
    assert!(shown.contains("hooks/pre-push"), "{shown}");
    assert!(out.status.success(), "{shown}");
    assert!(shown.contains("nothing changed"), "{shown}");
    // Declining a repair is not denying the permission, and nothing was written.
    let status = m.status(&m.f.repo);
    assert_eq!(status.permission, Permission::Granted);
    assert_eq!(status.state, ProtectionState::Unprotected);
    assert_eq!(hooks(&m)["status"], "inactive");
    assert_eq!(
        m.git_ok(&m.f.repo, &["config", "--local", "core.hooksPath"]),
        ".husky/_"
    );
}

#[test]
fn the_permission_screen_is_in_spanish_too() {
    let m = taken_over();
    // The pty of `script` runs with the environment of the machine: Spanish through `LANG`.
    m.extra_env
        .borrow_mut()
        .push(("LANG", "es_ES.UTF-8".into()));
    let out = m.developer_answering(
        &["guard", "install", m.f.repo.to_str().unwrap()],
        "[s/N]",
        "n\n",
    );
    let shown = text(&out);
    assert!(shown.contains("Reparar la protección de"), "{shown}");
}

mod cuts {
    use super::*;
    use gitraptor_testkit::cut::{ENV_CUT, points};

    const STEPS: &[&str] = &["repair-files", "repair-key"];

    #[test]
    fn a_repair_cut_at_any_point_is_complete_or_as_it_was() {
        let mut broken = Vec::new();
        for point in points(STEPS, &[]) {
            let m = taken_over();
            m.stop();
            let config = std::fs::read(m.common().join("config")).unwrap();
            let husky = std::fs::read_to_string(m.f.repo.join(".husky/_/pre-commit")).unwrap();
            m.extra_env
                .borrow_mut()
                .push((ENV_CUT, point.to_string().into()));
            let out = m.protect(&m.f.repo);
            assert!(!out.status.success(), "{point}: {}", text(&out));
            m.extra_env.borrow_mut().clear();
            // The daemon died at the point; the next start recovers before serving.
            let out = m.raptor(&["daemon", "status"]);
            assert!(out.status.success(), "{point}: {}", text(&out));

            let now = hooks(&m);
            let husky_after =
                std::fs::read_to_string(m.f.repo.join(".husky/_/pre-commit")).unwrap();
            if husky_after != husky {
                broken.push(format!("{point}: the other tool's hook changed"));
            }
            match now["status"].as_str() {
                // Complete: the key is ours and a force-push is denied again.
                Some("active") => {
                    let denied = !m.git(&m.f.repo, &["branch", "-D", "main"]).status.success();
                    if !denied {
                        broken.push(format!("{point}: active but branch -D main passed"));
                    }
                }
                // As it was: still the other tool's key, byte for byte.
                Some("inactive") => {
                    let same = std::fs::read(m.common().join("config")).unwrap() == config;
                    if !same || now["cause"] != "hookspath-changed" {
                        broken.push(format!("{point}: inactive but not as it was: {now:#}"));
                    }
                }
                other => broken.push(format!("{point}: unexpected state {other:?}")),
            }
            // Either way, installing again completes it.
            let out = m.protect(&m.f.repo);
            if !out.status.success() || hooks(&m)["status"] != "active" {
                broken.push(format!(
                    "{point}: the next install did not complete: {}",
                    text(&out)
                ));
            }
        }
        assert!(broken.is_empty(), "{broken:#?}");
    }
}
