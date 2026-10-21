//! US-GRD-014 end to end: the team fixes the base branch on the main branch and Guardrails
//! protects it once the developer confirms it with `raptor guard confirm [<path>]`. Real `raptor`
//! (daemon, client and hook), the real `raptor-hook` dispatcher and plain Git over the temporary
//! machine of `guard_machine` (NFR-01). The window is shortened with
//! `GITRAPTOR_TEST_GUARD_WINDOW_MS`; no fixed waits. Unix only; Windows has no channel transport
//! yet (Pendiente: etapa de validación multiplataforma).
//!
//! Wire names and texts these tests assume (kebab-case, per DS-US-GRD-014):
//! - `guard.status` JSON: `confirm: {"action": "guard-confirm"}` while something waits for a
//!   confirmation (absent otherwise and with an invalid base); diagnostics `base-unconfirmed`,
//!   `base-change-pending`, `floor-relax-pending`; `pending.action == "confirm-team"` during the
//!   window of a confirmation;
//! - the audit of reserved commands: operation `guard.confirm`, outcomes `applied`, `cancelled`,
//!   `changed`, and `rejected` for an agent's attempt;
//! - the CLI: the plan comes before the question `[y/N]` (default "no") and names the base
//!   change and `Relaxations (N)`; the window prints `Ctrl-C` and ends in `cancelled` when
//!   cancelled, like the uninstall; an agent gets "only the developer can change the
//!   protection"; a refusal (nothing to confirm, invalid base, unreadable floor, not observed,
//!   changed) never asks the question.
#![cfg(unix)]

mod guard_machine;

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use gitraptor_core::profile::Profile;
use guard_machine::{Machine, text};
use serde_json::Value;

#[test]
fn fake_agent_entry() {
    guard_machine::fake_agent_entry();
}

const DEVELOP: &str = r#"{"engine":{"baseBranch":"develop"}}"#;
const BASE_PROTECTION: &str = "Base branch protection";
const QUESTION: &str = "[y/N]";
const UNKNOWN_COMMAND: &str = "unrecognized subcommand";

fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Commits `settings` as the team settings on `main` of "demo" and pushes it, so it reaches the
/// copy of the main branch (`origin/main`). Leaves "demo" on `main`.
fn publish(m: &Machine, settings: &str) {
    m.git_ok(&m.f.repo, &["switch", "-q", "main"]);
    let dir = m.f.repo.join(".gitraptor");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.json"), settings).unwrap();
    m.git_ok(&m.f.repo, &["add", ".gitraptor/settings.json"]);
    m.git_ok(&m.f.repo, &["commit", "-q", "-m", "team settings"]);
    m.git_ok(&m.f.repo, &["push", "-q", "origin", "main"]);
}

/// Creates the local branches `names` at `main` and leaves "demo" on `feat-x`, so `main` and
/// them can be deleted.
fn branches(m: &Machine, names: &[&str]) {
    for name in names {
        m.git_ok(&m.f.repo, &["branch", "-f", name, "main"]);
    }
    m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
}

fn protect(m: &Machine, path: &Path) {
    let out = m.protect(path);
    assert!(out.status.success(), "{}", text(&out));
}

/// The `guard status` JSON once `ready` holds (the daemon re-reads the team level on a ref
/// change; the signal is the status itself).
fn status_when(m: &Machine, path: &Path, ready: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status = m.status_json(path);
        if ready(&status) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "status never got ready: {status:#}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn has_diagnostic(status: &Value, code: &str) -> bool {
    status["diagnostics"]
        .as_array()
        .is_some_and(|d| d.iter().any(|x| x == code))
}

fn bases(status: &Value) -> Vec<String> {
    status["protected_bases"]
        .as_array()
        .unwrap_or_else(|| panic!("no protected_bases: {status:#}"))
        .iter()
        .map(|b| b["untrusted"].as_str().unwrap().to_owned())
        .collect()
}

fn offers_confirm(status: &Value) -> bool {
    status["confirm"]["action"] == "guard-confirm"
}

/// Deleting `branch` from `dir` is denied by the base branch protection, naming it.
fn assert_base_denied(m: &Machine, dir: &Path, branch: &str) {
    let refname = format!("refs/heads/{branch}");
    let before = m.git_ok(dir, &["rev-parse", &refname]);
    let out = m.git(dir, &["branch", "-D", branch]);
    let shown = text(&out);
    assert!(!out.status.success(), "deleting {branch}: {shown}");
    assert!(
        shown.contains(BASE_PROTECTION) && shown.contains(branch),
        "deleting {branch}: {shown}"
    );
    assert_eq!(m.git_ok(dir, &["rev-parse", &refname]), before);
}

/// Deleting `branch` from `dir` gets the decision of any unprotected branch: it goes ahead.
fn assert_unprotected_delete(m: &Machine, dir: &Path, branch: &str) {
    let out = m.git(dir, &["branch", "-D", branch]);
    let shown = text(&out);
    assert!(out.status.success(), "deleting {branch}: {shown}");
    assert!(
        !shown.contains(BASE_PROTECTION),
        "deleting {branch}: {shown}"
    );
}

/// `raptor guard confirm <path>` from the developer's terminal, answering yes to the plan; the
/// short window of the machine closes and the confirmation applies.
fn confirm_yes(m: &Machine, path: &Path) -> String {
    let out = m.developer_answering(&["guard", "confirm", p(path)], QUESTION, "y\n");
    let shown = text(&out);
    assert!(out.status.success(), "{shown}");
    shown
}

/// `raptor guard confirm <path>` that must end without asking (a refusal or nothing to
/// confirm): the command exists and never shows the question.
fn confirm_without_question(m: &Machine, path: &Path) -> Output {
    let out = m.developer(&["guard", "confirm", p(path)]);
    let shown = text(&out);
    assert!(!shown.contains(UNKNOWN_COMMAND), "{shown}");
    assert!(!shown.contains(QUESTION), "{shown}");
    out
}

/// What the store of the repo at `common` keeps as confirmed: the base branch name, if any.
/// Stops the daemon to read the profile.
fn confirmed_base(m: &Machine, common: &Path) -> Option<String> {
    m.stop();
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    let entry = profile.repo_by_common_dir(common).unwrap().unwrap();
    let (store, _) = profile.open_store(&entry.repo_id).unwrap();
    store
        .confirmed_team_baseline()
        .unwrap()
        .map(|c| c.base_branch.to_string())
}

/// Outcomes of `guard.confirm` in the audit of reserved commands. Stops the daemon.
fn confirm_audit(m: &Machine) -> Vec<String> {
    m.stop();
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    profile
        .audit(0, 1000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.operation == "guard.confirm")
        .map(|(_, row)| row.outcome)
        .collect()
}

/// "demo" protected with `develop` in the team settings, not confirmed; on `feat-x`.
fn unconfirmed_develop(m: &Machine) {
    publish(m, DEVELOP);
    branches(m, &["develop"]);
    m.add(&m.f.repo);
    protect(m, &m.f.repo);
}

/// "demo" protected with `main` confirmed (no team settings), then `develop` reaches the main
/// branch: a pending change. On `feat-x`.
fn pending_develop(m: &Machine) -> Value {
    m.add(&m.f.repo);
    protect(m, &m.f.repo);
    let status = m.status_json(&m.f.repo);
    assert_eq!(status["base_confirmed"], true, "{status:#}");
    publish(m, DEVELOP);
    branches(m, &["develop"]);
    status_when(m, &m.f.repo, |s| has_diagnostic(s, "base-change-pending"))
}

// Escenario 1 · Sin confirmación inicial, la rama base del equipo y main quedan protegidas hasta
// que el desarrollador confirma.
#[test]
fn unconfirmed_team_base_guards_the_union_until_confirmed() {
    let m = Machine::new();
    unconfirmed_develop(&m);
    assert_base_denied(&m, &m.f.repo, "develop");
    assert_base_denied(&m, &m.f.repo, "main");
    let status = m.status_json(&m.f.repo);
    assert_eq!(status["base_confirmed"], false, "{status:#}");
    assert!(has_diagnostic(&status, "base-unconfirmed"), "{status:#}");
    assert!(offers_confirm(&status), "{status:#}");
    let shown = bases(&status);
    assert!(
        shown.contains(&"develop".to_owned()) && shown.contains(&"main".to_owned()),
        "{shown:?}"
    );

    confirm_yes(&m, &m.f.repo);
    let status = m.status_json(&m.f.repo);
    assert_eq!(bases(&status), ["develop"], "{status:#}");
    assert_eq!(status["base_confirmed"], true, "{status:#}");
    assert!(!has_diagnostic(&status, "base-unconfirmed"), "{status:#}");
    assert!(status.get("confirm").is_none(), "{status:#}");
    assert_base_denied(&m, &m.f.repo, "develop");
    assert_unprotected_delete(&m, &m.f.repo, "main");
    assert_eq!(confirmed_base(&m, &m.common()).as_deref(), Some("develop"));
}

// Escenario 1, riesgo real (D7, D12) · El perfil se perdió con los hooks instalados, sin adoptar.
#[test]
fn a_lost_profile_still_guards_the_team_base_and_main() {
    let m = Machine::new();
    unconfirmed_develop(&m);
    m.stop();
    // The profile is lost: an empty one in its place.
    std::fs::remove_dir_all(&m.f.profile).unwrap();
    use std::os::unix::fs::PermissionsExt;
    for dir in ["", "data", "config", "state"] {
        let d = m.f.profile.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert!(
        m.common()
            .join("gitraptor/hooks/reference-transaction")
            .exists()
    );
    assert_base_denied(&m, &m.f.repo, "develop");
    assert_base_denied(&m, &m.f.repo, "main");
}

// Escenario 2 · Sin rama base del equipo, la rama base es main.
#[test]
fn without_a_team_base_main_is_guarded() {
    let m = Machine::new();
    branches(&m, &[]);
    m.add(&m.f.repo);
    protect(&m, &m.f.repo);
    assert_base_denied(&m, &m.f.repo, "main");
    let status = m.status_json(&m.f.repo);
    assert_eq!(bases(&status), ["main"], "{status:#}");
    assert_eq!(status["base_confirmed"], true, "{status:#}");
    assert!(status.get("confirm").is_none(), "{status:#}");
    confirm_without_question(&m, &m.f.repo);
    assert_eq!(confirm_audit(&m), Vec::<String>::new());
    assert_eq!(confirmed_base(&m, &m.common()).as_deref(), Some("main"));
}

// Escenario 3 · Un nivel personal no cambia la rama base.
#[test]
fn a_personal_base_branch_changes_nothing() {
    let m = Machine::new();
    publish(&m, DEVELOP);
    branches(&m, &["develop", "release"]);
    m.add(&m.f.repo);
    protect(&m, &m.f.repo);
    confirm_yes(&m, &m.f.repo);
    let status = m.status_json(&m.f.repo);
    let repo_id = status["repo_id"].as_str().unwrap().to_owned();
    let local = m
        .dirs()
        .config
        .join("repos")
        .join(&repo_id)
        .join("settings.local.json");
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(&local, r#"{"engine":{"baseBranch":"release"}}"#).unwrap();
    // The daemon reads the local level again on its next start.
    m.stop();
    let out = m.raptor(&["daemon", "status"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_unprotected_delete(&m, &m.f.repo, "release");
    let status = m.status_json(&m.f.repo);
    assert_eq!(bases(&status), ["develop"], "{status:#}");
    assert_eq!(status["base_confirmed"], true, "{status:#}");
    assert_base_denied(&m, &m.f.repo, "develop");
}

// Escenario 4 · Todos los worktrees comparten la rama base de la rama principal.
#[test]
fn every_worktree_guards_the_main_branch_base() {
    let m = Machine::new();
    publish(&m, DEVELOP);
    branches(&m, &["develop", "release"]);
    let worktree = |name: &str, settings: &str| -> PathBuf {
        let dir = m.f.root.join(name);
        m.git_ok(
            &m.f.repo,
            &["worktree", "add", "-q", "-b", name, p(&dir), "main"],
        );
        std::fs::write(dir.join(".gitraptor/settings.json"), settings).unwrap();
        std::fs::write(dir.join(format!("{name}.txt")), name).unwrap();
        m.git_ok(&dir, &["add", "."]);
        m.git_ok(&dir, &["commit", "-q", "-m", name]);
        dir
    };
    let feat_a = worktree("feat-a", DEVELOP);
    let feat_b = worktree("feat-b", r#"{"engine":{"baseBranch":"release"}}"#);
    m.add(&m.f.repo);
    protect(&m, &m.f.repo);
    confirm_yes(&m, &m.f.repo);
    assert_eq!(bases(&m.status_json(&m.f.repo)), ["develop"]);

    assert_base_denied(&m, &feat_a, "develop");
    assert_base_denied(&m, &feat_b, "develop");
    assert_unprotected_delete(&m, &feat_b, "release");
}

// Escenario 5 · En una máquina nueva aplica desde el primer momento.
#[test]
fn a_new_machine_guards_the_team_base_from_the_first_operation() {
    let m = Machine::new();
    publish(&m, DEVELOP);
    m.git_ok(&m.f.repo, &["push", "-q", "origin", "main:develop"]);
    let clone = m.f.root.join("demo-clone");
    m.git_ok(&m.f.root, &["clone", "-q", p(&m.remote), p(&clone)]);
    m.git_ok(&clone, &["branch", "develop", "origin/develop"]);
    // An empty profile: nothing of this machine knows "demo".
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    assert!(
        profile
            .repo_by_common_dir(&clone.join(".git"))
            .unwrap()
            .is_none()
    );
    drop(profile);

    m.add(&clone);
    protect(&m, &clone);
    confirm_yes(&m, &clone);
    assert_base_denied(&m, &clone, "develop");
    let status = m.status_json(&clone);
    assert_eq!(bases(&status), ["develop"], "{status:#}");
    assert_eq!(status["base_confirmed"], true, "{status:#}");
}

// Escenario 6 · Un cambio de rama base en la rama principal no se aplica hasta que el
// desarrollador lo confirma.
#[test]
fn a_base_change_waits_for_the_developer() {
    let m = Machine::new();
    let status = pending_develop(&m);
    assert_base_denied(&m, &m.f.repo, "main");
    assert_base_denied(&m, &m.f.repo, "develop");
    assert!(offers_confirm(&status), "{status:#}");
    let shown = bases(&status);
    assert!(
        shown.contains(&"develop".to_owned()) && shown.contains(&"main".to_owned()),
        "{shown:?}"
    );

    confirm_yes(&m, &m.f.repo);
    let status = m.status_json(&m.f.repo);
    assert_eq!(bases(&status), ["develop"], "{status:#}");
    assert!(
        !has_diagnostic(&status, "base-change-pending"),
        "{status:#}"
    );
    assert!(status.get("confirm").is_none(), "{status:#}");
    assert_base_denied(&m, &m.f.repo, "develop");
    assert_unprotected_delete(&m, &m.f.repo, "main");
    assert!(confirm_audit(&m).contains(&"applied".to_owned()));
}

// Escenario 6 · Si un agente intenta confirmarlo, se rechaza y el intento queda en el registro.
#[test]
fn an_agent_cannot_confirm_and_is_logged() {
    let m = Machine::new();
    pending_develop(&m);
    let out = m.as_agent(&["guard", "confirm", p(&m.f.repo)]);
    let shown = text(&out);
    assert!(!out.status.success(), "{shown}");
    assert!(
        shown.contains("only the developer can change the protection"),
        "{shown}"
    );
    let status = m.status_json(&m.f.repo);
    assert!(has_diagnostic(&status, "base-change-pending"), "{status:#}");
    assert!(status.get("pending").is_none(), "{status:#}");
    assert_base_denied(&m, &m.f.repo, "main");
    let outcomes = confirm_audit(&m);
    assert!(outcomes.contains(&"rejected".to_owned()), "{outcomes:?}");
    assert!(!outcomes.contains(&"applied".to_owned()), "{outcomes:?}");
    assert_eq!(confirmed_base(&m, &m.common()).as_deref(), Some("main"));
}

// D2 · El plan va antes de cualquier efecto: sin aceptarlo, nada se escribe ni se anuncia.
#[test]
fn the_plan_comes_before_any_effect() {
    let m = Machine::with_window(60_000);
    unconfirmed_develop(&m);
    for answer in ["n\n", "\n"] {
        let mut run = m.developer_spawn(&["guard", "confirm", p(&m.f.repo)]);
        run.wait_for(QUESTION);
        // The plan is on the terminal before the question: the base change, main → develop.
        let plan = String::from_utf8_lossy(&run.seen).into_owned();
        assert!(plan.contains("develop") && plan.contains("main"), "{plan}");
        run.type_line(answer);
        let out = run.finish();
        let shown = text(&out);
        assert!(!shown.contains("Ctrl-C"), "{answer:?}: {shown}");
        let status = m.status_json(&m.f.repo);
        assert!(status.get("pending").is_none(), "{answer:?}: {status:#}");
        assert_eq!(status["base_confirmed"], false, "{status:#}");
        assert!(offers_confirm(&status), "{status:#}");
    }
    assert_base_denied(&m, &m.f.repo, "main");
    let outcomes = confirm_audit(&m);
    assert!(!outcomes.contains(&"applied".to_owned()), "{outcomes:?}");
    assert_eq!(confirmed_base(&m, &m.common()), None);
}

// D4 · La primera confirmación de develop (referencia main) abre la ventana; cancelarla no
// confirma nada.
#[test]
fn confirming_a_base_other_than_main_waits_for_the_window() {
    let m = Machine::with_window(60_000);
    unconfirmed_develop(&m);
    let mut run = m.developer_spawn(&["guard", "confirm", p(&m.f.repo)]);
    run.wait_for(QUESTION);
    run.type_line("y\n");
    run.wait_for("Ctrl-C");
    let status = m.status_json(&m.f.repo);
    assert_eq!(status["pending"]["action"], "confirm-team", "{status:#}");
    let cancel = m.developer(&["guard", "cancel", p(&m.f.repo)]);
    assert!(cancel.status.success(), "{}", text(&cancel));
    let out = run.finish();
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("cancelled"), "{}", text(&out));
    let status = m.status_json(&m.f.repo);
    assert!(status.get("pending").is_none(), "{status:#}");
    assert_eq!(status["base_confirmed"], false, "{status:#}");
    assert_base_denied(&m, &m.f.repo, "main");
    let outcomes = confirm_audit(&m);
    assert!(outcomes.contains(&"cancelled".to_owned()), "{outcomes:?}");
    assert!(!outcomes.contains(&"applied".to_owned()), "{outcomes:?}");
    assert_eq!(confirmed_base(&m, &m.common()), None);
}

/// The `N` of the first `Relaxations (N)` on the terminal.
fn relaxations_shown(seen: &str) -> Option<u32> {
    let rest = &seen[seen.find("Relaxations (")? + "Relaxations (".len()..];
    rest[..rest.find(')')?].trim().parse().ok()
}

// D3/D4 · Un suelo que desactiva el mínimo seguro muestra la relajación y espera la ventana.
#[test]
fn a_relaxing_floor_is_shown_and_waits() {
    let m = Machine::with_window(60_000);
    m.add(&m.f.repo);
    protect(&m, &m.f.repo);
    publish(&m, r#"{"permissions":{"disableSafeMinimum":true}}"#);
    branches(&m, &[]);
    let status = status_when(&m, &m.f.repo, |s| has_diagnostic(s, "floor-relax-pending"));
    assert!(offers_confirm(&status), "{status:#}");

    let mut run = m.developer_spawn(&["guard", "confirm", p(&m.f.repo)]);
    run.wait_for(QUESTION);
    let plan = String::from_utf8_lossy(&run.seen).into_owned();
    let n = relaxations_shown(&plan).unwrap_or_else(|| panic!("no Relaxations (N): {plan}"));
    assert!(n > 0, "{plan}");
    assert!(plan.contains("disableSafeMinimum"), "{plan}");
    run.type_line("y\n");
    run.wait_for("Ctrl-C");
    let status = m.status_json(&m.f.repo);
    assert_eq!(status["pending"]["action"], "confirm-team", "{status:#}");
    let cancel = m.developer(&["guard", "cancel", p(&m.f.repo)]);
    assert!(cancel.status.success(), "{}", text(&cancel));
    let out = run.finish();
    assert!(text(&out).contains("cancelled"), "{}", text(&out));
    // The minimum still holds: the relaxation was not confirmed.
    m.rewrite_feat_x();
    let push = m.git(&m.f.repo, &["push", "-q", "-f", "origin", "feat-x"]);
    assert!(!push.status.success(), "{}", text(&push));
}

// D5 · Si la rama principal cambia el suelo entre el plan y la aplicación, no se confirma.
#[test]
fn a_floor_changed_during_the_window_is_not_confirmed() {
    let m = Machine::new();
    pending_develop(&m);
    let mut run = m.developer_spawn(&["guard", "confirm", p(&m.f.repo)]);
    run.wait_for(QUESTION);
    // The floor moves after the plan was shown.
    publish(
        &m,
        r#"{"engine":{"baseBranch":"develop"},"permissions":{"deny":["merge"]}}"#,
    );
    m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
    run.type_line("y\n");
    let out = run.finish();
    assert!(!out.status.success(), "{}", text(&out));
    let status = m.status_json(&m.f.repo);
    assert!(status.get("pending").is_none(), "{status:#}");
    assert!(has_diagnostic(&status, "base-change-pending"), "{status:#}");
    assert!(offers_confirm(&status), "{status:#}");
    assert_base_denied(&m, &m.f.repo, "main");
    let outcomes = confirm_audit(&m);
    assert!(!outcomes.contains(&"applied".to_owned()), "{outcomes:?}");
    assert_eq!(confirmed_base(&m, &m.common()).as_deref(), Some("main"));
}

// D5 · Una confirmación concurrente desde otro cliente se detecta: la segunda no escribe.
#[test]
fn a_concurrent_confirmation_is_detected() {
    let m = Machine::new();
    pending_develop(&m);
    // The first client has the plan on screen and has not answered yet.
    let mut first = m.developer_spawn(&["guard", "confirm", p(&m.f.repo)]);
    first.wait_for(QUESTION);
    // Another client confirms first.
    confirm_yes(&m, &m.f.repo);
    assert_eq!(bases(&m.status_json(&m.f.repo)), ["develop"]);
    first.type_line("y\n");
    let out = first.finish();
    let shown = text(&out);
    assert!(!out.status.success(), "{shown}");
    let status = m.status_json(&m.f.repo);
    assert!(status.get("pending").is_none(), "{status:#}");
    assert_eq!(bases(&status), ["develop"], "{status:#}");
    let outcomes = confirm_audit(&m);
    let applied = outcomes.iter().filter(|o| *o == "applied").count();
    assert_eq!(applied, 1, "{outcomes:?}");
    assert_eq!(confirmed_base(&m, &m.common()).as_deref(), Some("develop"));
}

// D4/D8 · Una baseBranch inválida no se confirma y el estado no ofrece la acción.
#[test]
fn an_invalid_team_base_cannot_be_confirmed() {
    let m = Machine::new();
    publish(&m, r#"{"engine":{"baseBranch":"--upload-pack=x"}}"#);
    branches(&m, &["develop"]);
    m.add(&m.f.repo);
    protect(&m, &m.f.repo);
    let status = m.status_json(&m.f.repo);
    assert!(status.get("confirm").is_none(), "{status:#}");
    let out = confirm_without_question(&m, &m.f.repo);
    assert!(!out.status.success(), "{}", text(&out));
    assert_base_denied(&m, &m.f.repo, "main");

    // Once the team fixes the value, the same command confirms it.
    publish(&m, DEVELOP);
    m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
    status_when(&m, &m.f.repo, offers_confirm);
    confirm_yes(&m, &m.f.repo);
    assert_eq!(bases(&m.status_json(&m.f.repo)), ["develop"]);
    assert_eq!(confirmed_base(&m, &m.common()).as_deref(), Some("develop"));
}

// D4 · Con el suelo ilegible o parcial, guard confirm no escribe.
#[test]
fn an_unreadable_floor_cannot_be_confirmed() {
    let m = Machine::new();
    publish(&m, "{ not json");
    branches(&m, &["develop"]);
    m.add(&m.f.repo);
    protect(&m, &m.f.repo);
    let out = confirm_without_question(&m, &m.f.repo);
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(m.status_json(&m.f.repo)["base_confirmed"], false);

    // A partial floor (an unknown policy) is refused too.
    publish(
        &m,
        r#"{"engine":{"baseBranch":"develop"},"policies":{"maxDiffLines":10}}"#,
    );
    m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
    let out = confirm_without_question(&m, &m.f.repo);
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(m.status_json(&m.f.repo)["base_confirmed"], false);
    assert_base_denied(&m, &m.f.repo, "main");

    // A readable floor is confirmed by the same command.
    publish(&m, DEVELOP);
    m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
    status_when(&m, &m.f.repo, offers_confirm);
    confirm_yes(&m, &m.f.repo);
    assert_eq!(confirmed_base(&m, &m.common()).as_deref(), Some("develop"));
}

// D11 · Un repo observado sin hooks ("Solo MCP") confirma su rama base; uno no observado, no.
#[test]
fn a_mcp_only_repo_can_confirm_its_base() {
    let m = Machine::new();
    publish(&m, DEVELOP);
    branches(&m, &["develop"]);
    m.add(&m.f.repo);
    assert!(!m.common().join("gitraptor").exists());
    confirm_yes(&m, &m.f.repo);
    assert!(!m.common().join("gitraptor").exists());

    let out = confirm_without_question(&m, &m.f.other_repo);
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(confirmed_base(&m, &m.common()).as_deref(), Some("develop"));
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    assert!(
        profile
            .repo_by_common_dir(&m.f.other_repo.join(".git"))
            .unwrap()
            .is_none()
    );
}

// D8 · Tras confirmar y reiniciar el daemon, el estado y la foto de los hooks conservan la
// confirmada.
#[test]
fn the_confirmed_base_survives_a_daemon_restart() {
    let m = Machine::new();
    unconfirmed_develop(&m);
    confirm_yes(&m, &m.f.repo);
    let repo_id = m.status_json(&m.f.repo)["repo_id"]
        .as_str()
        .unwrap()
        .to_owned();
    m.stop();
    let out = m.raptor(&["daemon", "status"]);
    assert!(out.status.success(), "{}", text(&out));
    let status = m.status_json(&m.f.repo);
    assert_eq!(bases(&status), ["develop"], "{status:#}");
    assert_eq!(status["base_confirmed"], true, "{status:#}");
    let snapshot = m
        .dirs()
        .state
        .join("guardrails")
        .join(format!("{repo_id}.json"));
    let snapshot: Value = serde_json::from_slice(&std::fs::read(&snapshot).unwrap()).unwrap();
    assert_eq!(snapshot["confirmed_base"], "develop", "{snapshot:#}");
    assert_base_denied(&m, &m.f.repo, "develop");
    assert_unprotected_delete(&m, &m.f.repo, "main");
}
