//! UX7 (brief guard-exec-ux): `raptor agent correct` on a worktree with no session GitRaptor
//! detected fails and says why (it only corrects detected sessions) and what to do (`raptor
//! sessions` to see them, `raptor agent register` to attribute work without a detected session),
//! in English and Spanish. Real `raptor` over the temporary machine of `guard_machine`
//! (NFR-01: temporary repo, profile and daemon); no agent runs, so nothing is detected.
//! Unix only.
#![cfg(unix)]

mod guard_machine;

use std::ffi::OsString;
use std::process::Output;

use guard_machine::{Machine, text};

fn correct(m: &Machine) -> Output {
    let repo = m.f.repo.to_string_lossy().into_owned();
    m.developer(&["agent", "correct", "Codex", "--worktree", &repo])
}

#[test]
fn correct_without_detected_session_explains_why_and_what_to_do() {
    let m = Machine::new();
    m.add(&m.f.repo);

    let out = correct(&m);
    assert!(
        !out.status.success(),
        "correcting without a detected session should fail: {}",
        text(&out)
    );
    let shown = text(&out).to_lowercase();
    assert!(
        shown.contains("detected"),
        "UX7 (en): stderr should say it only corrects sessions GitRaptor detected: {shown}"
    );
    assert!(
        shown.contains("only") && shown.contains("session"),
        "UX7 (en): stderr should say why: it only corrects detected sessions: {shown}"
    );
    assert!(
        shown.contains("raptor sessions"),
        "UX7 (en): stderr should point to `raptor sessions`: {shown}"
    );
    assert!(
        shown.contains("raptor agent register"),
        "UX7 (en): stderr should point to `raptor agent register`: {shown}"
    );

    m.extra_env
        .borrow_mut()
        .push(("LANG", OsString::from("es_ES.UTF-8")));
    let out = correct(&m);
    assert!(!out.status.success(), "{}", text(&out));
    let shown = text(&out).to_lowercase();
    assert!(
        shown.contains("solo") && shown.contains("sesion") && shown.contains("detectad"),
        "UX7 (es): stderr debe decir que solo corrige sesiones detectadas por GitRaptor: {shown}"
    );
    assert!(
        shown.contains("raptor sessions"),
        "UX7 (es): stderr debe apuntar a `raptor sessions`: {shown}"
    );
    assert!(
        shown.contains("raptor agent register"),
        "UX7 (es): stderr debe apuntar a `raptor agent register`: {shown}"
    );
}
