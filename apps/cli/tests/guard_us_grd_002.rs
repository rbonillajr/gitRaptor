//! US-GRD-002 end to end: the hooks a repo already had keep running once it is protected. Real
//! `raptor` (daemon, client and hook), the real `raptor-hook` dispatcher and plain Git over the
//! temporary machine of `guard_machine` (NFR-01). Unix only; Windows has no channel transport yet
//! (Pendiente: etapa de validación multiplataforma).
#![cfg(unix)]

mod guard_machine;

use std::path::Path;

use gitraptor_api::guard::ProtectionState;
use gitraptor_testkit::{Exceptions, check};
use guard_machine::{Machine, install_exceptions, text};

#[test]
fn fake_agent_entry() {
    guard_machine::fake_agent_entry();
}

/// A linter of the developer: rejects the commit while `lint.txt` says `LINT-ERROR`.
const LINTER: &str = "#!/bin/sh\nif grep -q LINT-ERROR lint.txt 2>/dev/null; then\n  echo 'lint: LINT-ERROR found' >&2\n  exit 1\nfi\nexit 0\n";

fn commit(m: &Machine, dir: &Path, lint: &str) -> std::process::Output {
    std::fs::write(dir.join("lint.txt"), lint).unwrap();
    m.git_ok(dir, &["add", "lint.txt"]);
    m.git(dir, &["commit", "-q", "-m", "lint"])
}

mod repo_intact {
    use super::*;

    // E1 · El permiso informa de los hooks previos (BR-AUTH-002).
    #[test]
    fn e1_the_permission_names_the_prior_hooks() {
        let m = Machine::new();
        m.script(&m.common().join("hooks/pre-commit"), LINTER);
        m.add(&m.f.repo);
        let out = m.developer_answering(
            &["guard", "install", m.f.repo.to_str().unwrap()],
            "[y/N]",
            "y\n",
        );
        let shown = text(&out);
        assert!(out.status.success(), "{shown}");
        for expected in [
            "Hooks this repository already has: pre-commit",
            "They are kept and keep running",
        ] {
            assert!(
                shown.contains(expected),
                "missing {expected:?} in:\n{shown}"
            );
        }
        assert_eq!(m.status(&m.f.repo).state, ProtectionState::HooksOnly);
    }

    // E2 y E3 · Tras proteger, el hook previo y Guardrails actúan los dos, y el hook previo
    // conserva exactamente su contenido.
    #[test]
    fn e2_e3_an_own_hook_and_guardrails_both_act() {
        let m = Machine::new();
        let hook = m.common().join("hooks/pre-commit");
        m.script(&hook, LINTER);
        let content = std::fs::read(&hook).unwrap();
        m.add(&m.f.repo);
        let mut out = None;
        let report = check("e2", &m.f, &install_exceptions(), || {
            out = Some(m.protect(&m.f.repo));
        });
        let out = out.unwrap();
        assert!(out.status.success(), "{}", text(&out));
        report.assert_intact();
        assert_eq!(std::fs::read(&hook).unwrap(), content);

        let rejected = commit(&m, &m.f.repo, "LINT-ERROR\n");
        assert!(!rejected.status.success(), "{}", text(&rejected));
        assert!(text(&rejected).contains("lint: LINT-ERROR found"));
        let clean = commit(&m, &m.f.repo, "fine\n");
        assert!(clean.status.success(), "{}", text(&clean));

        m.rewrite_feat_x();
        let before = m.remote_ref("feat-x");
        let push = m.git(&m.f.repo, &["push", "-q", "-f", "origin", "feat-x"]);
        assert!(!push.status.success(), "{}", text(&push));
        assert_eq!(m.remote_ref("feat-x"), before);
    }

    // E2 · Un `core.hooksPath` previo (husky, relativo a la raíz del worktree): sus hooks siguen
    // corriendo, también uno que Guardrails no gobierna, con el entorno original de Git.
    #[test]
    fn e2_a_prior_hooks_path_keeps_running_with_the_original_environment() {
        let m = Machine::new();
        let log = m.f.root.join("hook-log");
        let logger = |name: &str| {
            format!(
                "#!/bin/sh\necho \"{name} $TEAM_HOOK_MARK\" >> '{}'\nexit 0\n",
                log.display()
            )
        };
        m.script(&m.f.repo.join(".husky/_/pre-commit"), &logger("pre-commit"));
        m.script(
            &m.f.repo.join(".husky/_/post-commit"),
            &logger("post-commit"),
        );
        std::fs::write(m.f.repo.join(".husky/.gitignore"), "_\n").unwrap();
        m.git_ok(&m.f.repo, &["config", "core.hooksPath", ".husky/_"]);
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        assert!(m.common().join("gitraptor/hooks/post-commit").is_file());

        // A variable of the environment Git runs with: `raptor hook` never sees it, the prior
        // hook must.
        std::fs::write(m.f.repo.join("lint.txt"), "fine\n").unwrap();
        m.git_ok(&m.f.repo, &["add", "lint.txt"]);
        let clean =
            m.f.git_command(&m.f.repo, &["commit", "-q", "-m", "lint"])
                .env("TEAM_HOOK_MARK", "from-git-env")
                .output()
                .unwrap();
        assert!(clean.status.success(), "{}", text(&clean));
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(logged.contains("pre-commit from-git-env"), "{logged}");
        assert!(logged.contains("post-commit from-git-env"), "{logged}");
        let out = m.git(&m.f.repo, &["branch", "-D", "feat-x"]);
        assert!(out.status.success(), "{}", text(&out));
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
    }

    // Encadenado de `reference-transaction` y `pre-push`: el hook previo recibe exactamente la
    // misma entrada y su resultado se respeta.
    #[test]
    fn a_chained_hook_gets_the_same_input_and_its_result_is_respected() {
        let m = Machine::new();
        let log = m.f.root.join("rt-log");
        m.script(
            &m.common().join("hooks/reference-transaction"),
            &format!(
                "#!/bin/sh\necho \"state $1\" >> '{0}'\ncat >> '{0}'\nexit 0\n",
                log.display()
            ),
        );
        m.script(
            &m.common().join("hooks/pre-push"),
            "#!/bin/sh\necho 'pre-push: refused by the team hook' >&2\nexit 1\n",
        );
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        m.git_ok(&m.f.repo, &["branch", "topic"]);
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(logged.contains("state prepared"), "{logged}");
        assert!(logged.contains("state committed"), "{logged}");
        let main = m.git_ok(&m.f.repo, &["rev-parse", "main"]);
        assert!(
            logged.contains(&format!(
                "0000000000000000000000000000000000000000 {main} refs/heads/topic"
            )),
            "{logged}"
        );
        // A push Guardrails allows still runs the team's pre-push, which refuses it.
        let push = m.git(&m.f.repo, &["push", "-q", "origin", "topic"]);
        assert!(!push.status.success(), "{}", text(&push));
        assert!(text(&push).contains("pre-push: refused by the team hook"));
        assert_eq!(m.remote_ref("topic"), None);
    }

    // Lo que Git dio al dispatcher llega igual al hook previo: argumentos, entrada, cwd y
    // salida. Un `pre-push` previo que lee la entrada y un `commit-msg` previo que edita el
    // mensaje (la edición se conserva).
    #[test]
    fn a_chained_hook_gets_argv_stdin_cwd_and_its_edits_survive() {
        let m = Machine::new();
        let log = m.f.root.join("push-log");
        m.script(
            &m.common().join("hooks/pre-push"),
            &format!(
                "#!/bin/sh\necho \"args $1 $2\" >> '{0}'\necho \"cwd $(pwd)\" >> '{0}'\ncat >> '{0}'\necho 'pre-push: team hook ran'\nexit 0\n",
                log.display()
            ),
        );
        m.script(
            &m.common().join("hooks/commit-msg"),
            "#!/bin/sh\nprintf '%s\\n\\nReviewed-by: team hook\\n' \"$(cat \"$1\")\" > \"$1\"\n",
        );
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));

        let clean = commit(&m, &m.f.repo, "fine\n");
        assert!(clean.status.success(), "{}", text(&clean));
        let message = m.git_ok(&m.f.repo, &["log", "-1", "--format=%B"]);
        assert!(message.contains("Reviewed-by: team hook"), "{message}");

        m.git_ok(&m.f.repo, &["branch", "topic"]);
        let topic = m.git_ok(&m.f.repo, &["rev-parse", "topic"]);
        let push = m.git(&m.f.repo, &["push", "origin", "topic"]);
        assert!(push.status.success(), "{}", text(&push));
        assert!(
            text(&push).contains("pre-push: team hook ran"),
            "{}",
            text(&push)
        );
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            logged.contains(&format!("args origin {}", m.remote.display())),
            "{logged}"
        );
        assert!(
            logged.contains(&format!("cwd {}", m.f.repo.display())),
            "{logged}"
        );
        assert!(
            logged.contains(&format!(
                "refs/heads/topic {topic} refs/heads/topic 0000000000000000000000000000000000000000"
            )),
            "{logged}"
        );
    }

    // Orden: GitRaptor decide primero. Si deniega, el hook previo no corre y se ve el motivo de
    // GitRaptor; si permite, el hook previo corre y todavía puede hacer fallar la operación.
    #[test]
    fn guardrails_decides_first_and_a_deny_never_runs_the_prior_hook() {
        let m = Machine::new();
        let log = m.f.root.join("ran-log");
        m.script(
            &m.common().join("hooks/pre-push"),
            &format!("#!/bin/sh\necho ran >> '{}'\nexit 0\n", log.display()),
        );
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        m.rewrite_feat_x();
        let push = m.git(&m.f.repo, &["push", "-q", "-f", "origin", "feat-x"]);
        assert!(!push.status.success(), "{}", text(&push));
        assert!(text(&push).contains("force-push"), "{}", text(&push));
        assert!(!log.exists(), "the prior hook ran on a deny");
    }

    // Como Git: un hook previo sin `#!` corre con `sh`; uno cuyo intérprete no existe hace
    // fallar la operación, nunca la deja pasar como si hubiera corrido.
    #[test]
    fn a_prior_hook_without_shebang_runs_and_a_broken_one_fails_the_operation() {
        let m = Machine::new();
        let log = m.f.root.join("plain-log");
        m.script(
            &m.common().join("hooks/pre-commit"),
            &format!("echo plain >> '{}'\n", log.display()),
        );
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        let clean = commit(&m, &m.f.repo, "fine\n");
        assert!(clean.status.success(), "{}", text(&clean));
        assert_eq!(std::fs::read_to_string(&log).unwrap_or_default(), "plain\n");
        m.script(
            &m.common().join("hooks/pre-commit"),
            "#!/nonexistent/interpreter\nexit 0\n",
        );
        let broken = commit(&m, &m.f.repo, "again\n");
        assert!(!broken.status.success(), "{}", text(&broken));
        assert!(
            text(&broken).contains("could not be run"),
            "{}",
            text(&broken)
        );
    }

    // E4 · Si no se puede encadenar, no se instala nada: las rutas operativas quedan idénticas
    // y el intento queda guardado como `chain-impossible`.
    #[test]
    fn e4_a_prior_hook_that_cannot_be_chained_installs_nothing() {
        let m = Machine::new();
        m.git_ok(&m.f.repo, &["config", "core.hooksPath", "hooks\tdir"]);
        m.add(&m.f.repo);
        let exceptions =
            Exceptions::none().and(gitraptor_testkit::Exceptions::engine_profile("profile"));
        let mut out = None;
        let report = check("e4", &m.f, &exceptions, || {
            out = Some(m.protect(&m.f.repo));
        });
        let out = out.unwrap();
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("cannot be chained"), "{}", text(&out));
        report.assert_intact();
        let status = m.status_json(&m.f.repo);
        assert_eq!(status["state"], "unprotected");
        assert_eq!(
            status["last_refusal"],
            serde_json::json!(["chain-impossible"])
        );
    }
}
