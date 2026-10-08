//! US-GRD-003 end to end: removing the protection leaves the repo exactly as it was. Real
//! `raptor` (daemon, client and hook), the real `raptor-hook` dispatcher and plain Git over the
//! temporary machine of `guard_machine` (NFR-01). The fingerprint of every scope of the machine
//! (the repo, the other repo, the home with the global Git configuration and the system
//! configuration) after the uninstall is compared with the one before the install. Unix only;
//! Windows has no channel transport yet (Pendiente: etapa de validación multiplataforma).
#![cfg(unix)]

mod guard_machine;

use gitraptor_api::guard::{Permission, ProtectionState};
use guard_machine::{Machine, text, uninstalled_exceptions};

#[test]
fn fake_agent_entry() {
    guard_machine::fake_agent_entry();
}

const LINTER: &str = "#!/bin/sh\nif grep -q LINT-ERROR lint.txt 2>/dev/null; then\n  echo 'lint: LINT-ERROR found' >&2\n  exit 1\nfi\nexit 0\n";

/// Protects "demo", removes the protection and checks that nothing differs from before the
/// install: every scope of the machine, and the common `config` byte by byte.
fn round_trip(m: &Machine) {
    m.add(&m.f.repo);
    let exceptions = uninstalled_exceptions();
    let before = m.f.snapshot(&exceptions);
    let config = std::fs::read(m.common().join("config")).unwrap();
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(m.status(&m.f.repo).state, ProtectionState::HooksOnly);

    let out = m.uninstall(&m.f.repo);
    let shown = text(&out);
    assert!(out.status.success(), "{shown}");
    assert!(shown.contains("Ctrl-C"), "{shown}");
    assert!(!m.common().join("gitraptor").exists());
    let after = m.f.snapshot(&exceptions);
    let changes = exceptions.filter(&gitraptor_testkit::diff(&before, &after), &before, &after);
    assert!(changes.is_empty(), "{changes:#?}");
    assert_eq!(std::fs::read(m.common().join("config")).unwrap(), config);
    let status = m.status(&m.f.repo);
    assert_eq!(status.state, ProtectionState::Unprotected);
}

fn commit(m: &Machine, lint: &str) -> std::process::Output {
    std::fs::write(m.f.repo.join("lint.txt"), lint).unwrap();
    m.git_ok(&m.f.repo, &["add", "lint.txt"]);
    m.git(&m.f.repo, &["commit", "-q", "-m", "lint"])
}

mod repo_intact {
    use super::*;

    // E1 · Desinstalar restaura el estado exacto (sin hooks previos).
    #[test]
    fn e1_uninstall_restores_the_exact_state() {
        let m = Machine::new();
        round_trip(&m);
    }

    // E1 · …con un hook propio de linter, que sigue funcionando.
    #[test]
    fn e1_uninstall_restores_the_exact_state_with_an_own_hook() {
        let m = Machine::new();
        m.script(&m.common().join("hooks/pre-commit"), LINTER);
        round_trip(&m);
        let rejected = commit(&m, "LINT-ERROR\n");
        assert!(!rejected.status.success(), "{}", text(&rejected));
        assert!(text(&rejected).contains("lint: LINT-ERROR found"));
    }

    // E1 · …con un `core.hooksPath` local previo (husky): vuelve el mismo valor, en el mismo
    // nivel y en la misma línea.
    #[test]
    fn e1_uninstall_restores_a_prior_local_hooks_path() {
        let m = Machine::new();
        m.script(&m.f.repo.join(".husky/_/pre-commit"), LINTER);
        std::fs::write(m.f.repo.join(".husky/.gitignore"), "_\n").unwrap();
        m.git_ok(&m.f.repo, &["config", "core.hooksPath", ".husky/_"]);
        round_trip(&m);
        assert_eq!(
            m.git_ok(&m.f.repo, &["config", "--local", "core.hooksPath"]),
            ".husky/_"
        );
        let rejected = commit(&m, "LINT-ERROR\n");
        assert!(!rejected.status.success(), "{}", text(&rejected));
    }

    // E4 · Nada cambia fuera del repo en ningún momento: ni la configuración global (el home
    // de la máquina temporal), ni la de sistema, ni el repo "otro", tras proteger ni tras
    // retirar. (Un `core.hooksPath` global previo no se prueba de punta a punta: el daemon lee
    // la configuración global de la cuenta, no la del home temporal; lo cubren los tests de
    // `guardrails::prior`.)
    #[test]
    fn e4_nothing_changes_outside_the_repo_at_any_moment() {
        let m = Machine::new();
        m.add(&m.f.repo);
        let exceptions = uninstalled_exceptions();
        let before = m.f.snapshot(&exceptions);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        let protected = m.f.snapshot(&exceptions);
        let outside: Vec<_> = gitraptor_testkit::diff(&before, &protected)
            .into_iter()
            .filter(|c| c.scope != "repo" && c.scope != "profile")
            .collect();
        assert!(outside.is_empty(), "{outside:#?}");
        let out = m.uninstall(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        let after = m.f.snapshot(&exceptions);
        let changes = exceptions.filter(&gitraptor_testkit::diff(&before, &after), &before, &after);
        assert!(changes.is_empty(), "{changes:#?}");
    }

    // E2 · Tras retirar la protección, el mínimo seguro deja de aplicarse con Git directo.
    #[test]
    fn e2_after_uninstall_a_force_push_is_not_evaluated() {
        let m = Machine::new();
        round_trip(&m);
        m.rewrite_feat_x();
        let push = m.git(&m.f.repo, &["push", "-q", "-f", "origin", "feat-x"]);
        assert!(push.status.success(), "{}", text(&push));
    }

    // ADR-GRD-007 § 1 (D5) · La ventana: cancelar mantiene la protección.
    #[test]
    fn the_window_can_be_cancelled_and_the_protection_stays() {
        let m = Machine::with_window(60_000);
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        let mut running =
            m.developer_spawn(&["guard", "uninstall", "--yes", m.f.repo.to_str().unwrap()]);
        running.wait_for("Ctrl-C");
        let cancel = m.developer(&["guard", "cancel", m.f.repo.to_str().unwrap()]);
        assert!(cancel.status.success(), "{}", text(&cancel));
        let out = running.finish();
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("cancelled"), "{}", text(&out));
        let status = m.status(&m.f.repo);
        assert_eq!(status.state, ProtectionState::HooksOnly);
        assert_eq!(status.permission, Permission::Granted);
        assert!(m.common().join("gitraptor/hooks/pre-push").is_file());
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
    }

    // BR-AUTH-001 · Un agente no puede desinstalar.
    #[test]
    fn an_agent_cannot_uninstall() {
        let m = Machine::new();
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        let out = m.as_agent(&["guard", "uninstall", "--yes", m.f.repo.to_str().unwrap()]);
        assert!(!out.status.success(), "{}", text(&out));
        // Refused by the daemon as a reserved command, not by an unknown subcommand.
        assert!(
            text(&out).contains("only the developer can change the protection"),
            "{}",
            text(&out)
        );
        assert_eq!(m.status(&m.f.repo).state, ProtectionState::HooksOnly);
        assert!(m.common().join("gitraptor/hooks/pre-push").is_file());
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
    }
}

/// Marks the confirmed install of "demo" as an uninstall that stopped half way.
fn unfinish_uninstall(m: &Machine) {
    use gitraptor_core::profile::Profile;
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    let entry = profile.repo_by_common_dir(&m.common()).unwrap().unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    let journal = store.guard_keys().unwrap().journal.unwrap();
    let uninstalling = journal.replace(r#""stage":"confirmed""#, r#""stage":"uninstalling""#);
    assert_ne!(journal, uninstalling);
    store
        .set_guard_keys(Some(Some(&uninstalling)), None, None, None)
        .unwrap();
}

mod recovery {
    use super::*;

    // ADR-GRD-001 § 4, NFR-01: an install undone at startup with the key ours puts back the
    // `core.hooksPath` the repo had (husky), never leaves the repo without it.
    #[test]
    fn repo_intact_an_install_undone_at_startup_restores_the_prior_local_key() {
        let m = Machine::new();
        m.script(&m.f.repo.join(".husky/_/pre-commit"), LINTER);
        std::fs::write(m.f.repo.join(".husky/.gitignore"), "_\n").unwrap();
        m.git_ok(&m.f.repo, &["config", "core.hooksPath", ".husky/_"]);
        m.add(&m.f.repo);
        let exceptions = uninstalled_exceptions();
        let before = m.f.snapshot(&exceptions);
        let config = std::fs::read(m.common().join("config")).unwrap();
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        m.stop();
        // An install that never recorded its folder: the next start undoes it.
        use gitraptor_core::profile::Profile;
        let (profile, _) = Profile::open(m.dirs()).unwrap();
        let entry = profile.repo_by_common_dir(&m.common()).unwrap().unwrap();
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        let journal: serde_json::Value =
            serde_json::from_str(&store.guard_keys().unwrap().journal.unwrap()).unwrap();
        let mut journal = journal;
        journal["stage"] = "installing".into();
        journal["folder"] = serde_json::Value::Null;
        store
            .set_guard_keys(
                Some(Some(&journal.to_string())),
                Some("not-asked"),
                None,
                None,
            )
            .unwrap();
        drop(store);
        drop(profile);
        let out = m.raptor(&["daemon", "status"]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(m.status(&m.f.repo).state, ProtectionState::Unprotected);
        assert_eq!(
            m.git_ok(&m.f.repo, &["config", "--local", "core.hooksPath"]),
            ".husky/_"
        );
        assert_eq!(std::fs::read(m.common().join("config")).unwrap(), config);
        let after = m.f.snapshot(&exceptions);
        let changes = exceptions.filter(&gitraptor_testkit::diff(&before, &after), &before, &after);
        assert!(changes.is_empty(), "{changes:#?}");
    }

    // ADR-GRD-001 § 4 · La clave ya estaba restaurada: al arrancar se borra la carpeta y el
    // repo queda como antes de instalar.
    #[test]
    fn repo_intact_an_unfinished_uninstall_after_the_key_completes_at_startup() {
        let m = Machine::new();
        m.add(&m.f.repo);
        let exceptions = uninstalled_exceptions();
        let before = m.f.snapshot(&exceptions);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        m.stop();
        unfinish_uninstall(&m);
        m.git_ok(&m.f.repo, &["config", "--unset", "core.hooksPath"]);
        let out = m.raptor(&["daemon", "status"]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(!m.common().join("gitraptor").exists());
        assert_eq!(m.status(&m.f.repo).state, ProtectionState::Unprotected);
        let after = m.f.snapshot(&exceptions);
        let changes = exceptions.filter(&gitraptor_testkit::diff(&before, &after), &before, &after);
        assert!(changes.is_empty(), "{changes:#?}");
    }

    // ADR-GRD-001 § 4 · La clave sigue siendo la nuestra: la protección sigue completa.
    #[test]
    fn repo_intact_an_unfinished_uninstall_before_the_key_keeps_the_protection() {
        let m = Machine::new();
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        m.stop();
        unfinish_uninstall(&m);
        let out = m.raptor(&["daemon", "status"]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(m.status(&m.f.repo).state, ProtectionState::HooksOnly);
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
    }
}

/// NFR-12, ADR-GRD-001 § 4 and Validación 3: the uninstall killed before, during and after each
/// step of the inverse transaction (the daemon dies at the point, `test-cuts` only), then the
/// recovery at the next start. The repo ends complete (still protected) or identical byte by
/// byte to what it was before the install, never half way.
#[cfg(feature = "test-cuts")]
mod cuts {
    use super::*;
    use gitraptor_testkit::cut::{CutPoint, ENV_CUT, ENV_TRACE, points};

    /// The steps of the inverse transaction, in order. No `during` point: the key is written by
    /// Git with its own lock and rename, and a Git killed mid-write is the install suite's case.
    const STEPS: &[&str] = &[
        "uninstall-journal",
        "uninstall-key",
        "uninstall-folder",
        "uninstall-clear",
    ];

    fn protected_machine() -> (Machine, gitraptor_testkit::Snapshot, Vec<u8>) {
        let m = Machine::new();
        m.script(&m.common().join("hooks/pre-commit"), LINTER);
        m.add(&m.f.repo);
        let before = m.f.snapshot(&uninstalled_exceptions());
        let config = std::fs::read(m.common().join("config")).unwrap();
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        m.stop();
        (m, before, config)
    }

    #[test]
    fn repo_intact_an_uninstall_cut_at_any_point_is_complete_or_undone() {
        // The uncut run passes every declared point, and only those.
        let (m, _, _) = protected_machine();
        let trace = m.f.root.join("cut-trace");
        m.extra_env
            .borrow_mut()
            .push((ENV_TRACE, trace.clone().into()));
        let out = m.uninstall(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        let reached: Vec<String> = std::fs::read_to_string(&trace)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        let declared: Vec<String> = points(STEPS, &[]).iter().map(ToString::to_string).collect();
        assert_eq!(reached, declared);
        drop(m);

        let mut broken = Vec::new();
        for point in points(STEPS, &[]) {
            let (m, before, config) = protected_machine();
            m.extra_env
                .borrow_mut()
                .push((ENV_CUT, point.to_string().into()));
            let out = m.uninstall(&m.f.repo);
            assert!(!out.status.success(), "{point}: {}", text(&out));
            m.extra_env.borrow_mut().clear();
            // The daemon died at the point; the next start recovers before serving.
            let out = m.raptor(&["daemon", "status"]);
            assert!(out.status.success(), "{point}: {}", text(&out));
            if let Some(why) = classify(&m, &before, &config, &point) {
                broken.push(why);
            }
        }
        assert!(broken.is_empty(), "{broken:#?}");
    }

    /// `None` when the repo is in one of the two allowed end states.
    fn classify(
        m: &Machine,
        before: &gitraptor_testkit::Snapshot,
        config: &[u8],
        point: &CutPoint,
    ) -> Option<String> {
        let status = m.status(&m.f.repo);
        match status.state {
            ProtectionState::HooksOnly => {
                // Complete: still protected, the folder and the key in place.
                let denied = !m.git(&m.f.repo, &["branch", "-D", "main"]).status.success();
                (!denied).then(|| format!("{point}: protected but branch -D main passed"))
            }
            ProtectionState::Unprotected => {
                let exceptions = uninstalled_exceptions();
                let after = m.f.snapshot(&exceptions);
                let changes =
                    exceptions.filter(&gitraptor_testkit::diff(before, &after), before, &after);
                let same_config = std::fs::read(m.common().join("config")).unwrap() == config;
                (!changes.is_empty() || !same_config).then(|| {
                    format!("{point}: unprotected but differs (config same: {same_config}): {changes:#?}")
                })
            }
        }
    }
}
