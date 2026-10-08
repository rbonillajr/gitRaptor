//! US-GRD-004 end to end: the developer learns that the protection of a repo stopped being
//! active. Real `raptor` (daemon, client and hook) over the temporary machine of `guard_machine`
//! (NFR-01): each typical way of breaking the protection shows its cause in `guard status`, the
//! decision log and the event stream; Guardrails' own changes stay silent; reinstalling clears it.
//! Every wait is on an explicit signal (the log, the status), never a fixed sleep. Unix only;
//! Windows has no channel transport yet (Pendiente: etapa de validación multiplataforma).
#![cfg(unix)]

mod guard_machine;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gitraptor_api::guard::ProtectionState;
use guard_machine::{Machine, text};
use serde_json::Value;

#[test]
fn fake_agent_entry() {
    guard_machine::fake_agent_entry();
}

/// A protected "demo".
fn protected() -> Machine {
    let m = Machine::new();
    m.add(&m.f.repo);
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    m
}

fn gitraptor_dir(m: &Machine) -> PathBuf {
    m.common().join("gitraptor")
}

fn dispatcher(m: &Machine) -> PathBuf {
    gitraptor_dir(m).join("hooks/reference-transaction")
}

/// `raptor guard log --json` of "demo" (the pty of `script` may echo control characters).
fn log(m: &Machine) -> Value {
    let out = m.developer(&["guard", "log", "--json", m.f.repo.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = stdout
        .lines()
        .find_map(|l| l.find('{').map(|i| l[i..].to_owned()))
        .unwrap_or_else(|| panic!("no JSON: {stdout}"));
    serde_json::from_str(line.trim()).unwrap()
}

/// The `protection-state` entries of the log.
fn transitions(m: &Machine) -> Vec<Value> {
    log(m)["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "protection-state")
        .cloned()
        .collect()
}

/// Waits (on the log, which only the daemon's own check writes) for a transition to `to`, the
/// explicit signal that the loss was detected without anybody asking for the status.
fn wait_transition(m: &Machine, to: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(e) = transitions(m)
            .into_iter()
            .find(|e| e["operation"]["to"] == to)
        {
            return e;
        }
        assert!(
            Instant::now() < deadline,
            "no transition to {to}: {:#}",
            log(m)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn hooks(m: &Machine) -> Value {
    m.status_json(&m.f.repo)["hooks"].clone()
}

/// Breaks the protection one way, waits for the daemon to notice it and checks what it says.
fn assert_lost(m: &Machine, cause: &str) {
    let entry = wait_transition(m, "inactive");
    assert_eq!(entry["operation"]["cause"], cause, "{entry:#}");
    assert_eq!(entry["operation"]["expected"], false, "{entry:#}");
    let hooks = hooks(m);
    assert_eq!(hooks["status"], "inactive", "{hooks:#}");
    assert_eq!(hooks["cause"], cause, "{hooks:#}");
    assert_eq!(m.status(&m.f.repo).state, ProtectionState::Unprotected);
}

mod loss {
    use super::*;

    // E1 · Otra herramienta cambia `core.hooksPath`.
    #[test]
    fn another_manager_changes_the_hooks_path() {
        let m = protected();
        m.git_ok(&m.f.repo, &["config", "core.hooksPath", ".husky/_"]);
        assert_lost(&m, "hookspath-changed");
    }

    // E1 · Alguien borra la carpeta de Guardrails.
    #[test]
    fn the_folder_is_deleted() {
        let m = protected();
        std::fs::remove_dir_all(gitraptor_dir(&m)).unwrap();
        assert_lost(&m, "folder-missing");
    }

    // E1 · Alguien borra un dispatcher.
    #[test]
    fn a_dispatcher_is_deleted() {
        let m = protected();
        std::fs::remove_file(dispatcher(&m)).unwrap();
        assert_lost(&m, "dispatcher-missing");
    }

    // E1 · Otra herramienta reemplaza un dispatcher.
    #[test]
    fn a_dispatcher_is_replaced() {
        let m = protected();
        m.script(&dispatcher(&m), "#!/bin/sh\nexit 0\n");
        assert_lost(&m, "dispatcher-altered");
    }

    // E1 · Alguien quita el permiso de ejecución de un dispatcher.
    #[test]
    fn a_dispatcher_loses_its_execute_permission() {
        use std::os::unix::fs::PermissionsExt;
        let m = protected();
        let path = dispatcher(&m);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_lost(&m, "dispatcher-not-executable");
    }

    // ADR-GRD-005 Validación 3 · Editar solo el manifiesto no cambia el estado; el agente no
    // puede ocultar un dispatcher editado editando también el manifiesto.
    #[test]
    fn the_manifest_is_not_the_reference() {
        let m = protected();
        let manifest = gitraptor_dir(&m).join("manifest.json");
        let mut text = std::fs::read_to_string(&manifest).unwrap();
        text.push('\n');
        std::fs::write(&manifest, text).unwrap();
        assert_eq!(hooks(&m)["status"], "active");
        assert!(transitions(&m).is_empty(), "{:#}", log(&m));
        m.script(&dispatcher(&m), "#!/bin/sh\nexit 0\n");
        assert_lost(&m, "dispatcher-altered");
    }

    // La comprobación no escribe nada en el repo.
    #[test]
    fn the_check_writes_nothing() {
        let m = protected();
        let exceptions = guard_machine::install_exceptions();
        let before = m.f.snapshot(&exceptions);
        let _ = m.status(&m.f.repo);
        let after = m.f.snapshot(&exceptions);
        let changes = exceptions.filter(&gitraptor_testkit::diff(&before, &after), &before, &after);
        assert!(changes.is_empty(), "{changes:#?}");
    }
}

mod expected {
    use super::*;

    // E2 · Instalar y desinstalar desde Guardrails no avisan: quedan como transición esperada.
    #[test]
    fn guardrails_own_changes_do_not_alert() {
        let m = protected();
        let out = m.uninstall(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        let entry = wait_transition(&m, "not-installed");
        assert_eq!(entry["operation"]["expected"], true, "{entry:#}");
        assert_eq!(hooks(&m)["status"], "not-installed");
        assert!(
            transitions(&m)
                .iter()
                .all(|e| e["operation"]["expected"] == true),
            "{:#}",
            log(&m)
        );
        let text_out = m.raptor(&["guard", "status", m.f.repo.to_str().unwrap()]);
        assert!(!text(&text_out).contains("no longer active"), "{}", text(&text_out));
    }
}

mod repair {
    use super::*;

    /// Reinstalls and checks that the notice is gone.
    fn reinstall_clears(m: &Machine) {
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(hooks(m)["status"], "active", "{:#}", m.status_json(&m.f.repo));
        assert_eq!(m.status(&m.f.repo).state, ProtectionState::HooksOnly);
        let back = wait_transition(m, "active");
        assert_eq!(back["operation"]["cause"], Value::Null, "{back:#}");
    }

    #[test]
    fn reinstalling_after_the_hooks_path_changed() {
        let m = protected();
        m.script(&m.f.repo.join(".husky/_/pre-commit"), "#!/bin/sh\nexit 0\n");
        m.git_ok(&m.f.repo, &["config", "core.hooksPath", ".husky/_"]);
        assert_lost(&m, "hookspath-changed");
        reinstall_clears(&m);
        // The other manager's hooks are chained, not lost, and its file is untouched.
        assert!(m.f.repo.join(".husky/_/pre-commit").exists());
    }

    #[test]
    fn reinstalling_after_the_folder_was_deleted() {
        let m = protected();
        std::fs::remove_dir_all(gitraptor_dir(&m)).unwrap();
        assert_lost(&m, "folder-missing");
        reinstall_clears(&m);
    }

    #[test]
    fn reinstalling_after_a_dispatcher_was_replaced() {
        let m = protected();
        m.script(&dispatcher(&m), "#!/bin/sh\nexit 0\n");
        assert_lost(&m, "dispatcher-altered");
        reinstall_clears(&m);
    }

    #[test]
    fn reinstalling_after_a_dispatcher_was_deleted() {
        let m = protected();
        std::fs::remove_file(dispatcher(&m)).unwrap();
        assert_lost(&m, "dispatcher-missing");
        reinstall_clears(&m);
    }

    #[test]
    fn reinstalling_after_the_execute_permission_was_removed() {
        use std::os::unix::fs::PermissionsExt;
        let m = protected();
        std::fs::set_permissions(dispatcher(&m), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_lost(&m, "dispatcher-not-executable");
        reinstall_clears(&m);
    }

    // Nada de reinstalar en silencio: sin que el desarrollador lo pida, la pérdida sigue.
    #[test]
    fn nothing_is_reinstalled_in_silence() {
        let m = protected();
        std::fs::remove_file(dispatcher(&m)).unwrap();
        assert_lost(&m, "dispatcher-missing");
        assert!(!dispatcher(&m).exists());
        assert_eq!(hooks(&m)["status"], "inactive");
    }
}

mod debounce {
    use super::*;

    // Una causa persistente da una sola entrada agregada, no una por comprobación.
    #[test]
    fn a_persistent_cause_is_one_entry() {
        let m = protected();
        std::fs::remove_file(dispatcher(&m)).unwrap();
        assert_lost(&m, "dispatcher-missing");
        for _ in 0..3 {
            let _ = m.status(&m.f.repo);
        }
        let lost: Vec<_> = transitions(&m)
            .into_iter()
            .filter(|e| e["operation"]["to"] == "inactive")
            .collect();
        assert_eq!(lost.len(), 1, "{lost:#?}");
    }

    // D3 · Con el perfil borrado, el repo conserva clave y manifiesto: se muestra como huérfana.
    #[test]
    fn an_orphan_install_is_shown_as_orphaned() {
        let m = protected();
        m.stop();
        for dir in ["data", "state"] {
            let _ = std::fs::remove_dir_all(m.f.profile.join(dir));
        }
        let out = m.developer(&["repo", "add", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let hooks = hooks(&m);
        assert_eq!(hooks["status"], "orphaned", "{hooks:#}");
    }
}

mod surfaces {
    use super::*;

    // E1 · La CLI muestra la pérdida con su causa y la acción (los textos en/es los cubren los
    // tests de i18n).
    #[test]
    fn the_cli_shows_the_loss_and_the_action() {
        let m = protected();
        std::fs::remove_file(dispatcher(&m)).unwrap();
        assert_lost(&m, "dispatcher-missing");
        let repo = m.f.repo.to_str().unwrap();
        let en = text(&m.raptor(&["guard", "status", repo]));
        assert!(en.contains("raptor guard install"), "{en}");
        let status = text(&m.raptor(&["status"]));
        assert!(status.contains("raptor guard install"), "{status}");
    }

    // E4 · El mínimo seguro es visible: aplica el conjunto por defecto.
    #[test]
    fn the_minimum_set_is_visible() {
        let m = protected();
        let status = m.status_json(&m.f.repo);
        assert_eq!(status["minimum_set"]["status"], "active", "{status:#}");
    }
}
