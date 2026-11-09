//! UX6 (brief guard-exec-ux): the counters of `raptor guard status` and `raptor guard log` use the
//! singular with one item and the plural with several, in English and Spanish. Denials come from
//! plain force-pushes over the temporary machine of `guard_machine` (NFR-01: temporary repos,
//! remotes, profile and daemon). Unix only.
#![cfg(unix)]

mod guard_machine;

use std::ffi::OsString;

use guard_machine::{Machine, text};

fn repo(m: &Machine) -> String {
    m.f.repo.to_string_lossy().into_owned()
}

/// "demo" protected, with `feat-x` rewritten locally (a push needs a force), on `main`.
fn machine() -> Machine {
    let m = Machine::with_window(300);
    m.add(&m.f.repo);
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    m.rewrite_feat_x();
    m.git_ok(&m.f.repo, &["switch", "-q", "main"]);
    m
}

/// A force-push of feat-x by an unattributed process: denied and logged.
fn denied_force_push(m: &Machine) {
    let out = m.git(&m.f.repo, &["push", "-q", "-f", "origin", "feat-x"]);
    assert!(
        !out.status.success(),
        "the force-push should be denied: {}",
        text(&out)
    );
}

fn set_lang(m: &Machine, lang: &str) {
    m.extra_env
        .borrow_mut()
        .push(("LANG", OsString::from(lang)));
}

/// The text of `raptor guard status` and `raptor guard log` in the machine's language.
fn shown(m: &Machine) -> (String, String) {
    let status = m.developer(&["guard", "status", &repo(m)]);
    assert!(status.status.success(), "{}", text(&status));
    let log = m.developer(&["guard", "log", &repo(m)]);
    assert!(log.status.success(), "{}", text(&log));
    (text(&status), text(&log))
}

#[test]
fn one_item_uses_the_singular_in_en_and_es() {
    let m = machine();
    denied_force_push(&m);

    let (status, log) = shown(&m);
    for (name, shown) in [("guard status", &status), ("guard log", &log)] {
        assert!(
            shown.contains("1 blocked action") && !shown.contains("1 blocked actions"),
            "UX6 (en): {name} with one denial should say '1 blocked action': {shown}"
        );
    }

    set_lang(&m, "es_ES.UTF-8");
    let (status, log) = shown(&m);
    for (name, shown) in [("guard status", &status), ("guard log", &log)] {
        assert!(
            shown.contains("1 acción bloqueada") && !shown.contains("1 acciones bloqueadas"),
            "UX6 (es): {name} con un solo bloqueo debe decir '1 acción bloqueada': {shown}"
        );
    }

    // With several, the plural.
    denied_force_push(&m);
    let (status, log) = shown(&m);
    for (name, shown) in [("guard status", &status), ("guard log", &log)] {
        assert!(
            shown.contains("2 acciones bloqueadas"),
            "UX6 (es): {name} con dos bloqueos debe decir '2 acciones bloqueadas': {shown}"
        );
    }
    set_lang(&m, "en_US.UTF-8");
    let (status, log) = shown(&m);
    for (name, shown) in [("guard status", &status), ("guard log", &log)] {
        assert!(
            shown.contains("2 blocked actions"),
            "UX6 (en): {name} with two denials should say '2 blocked actions': {shown}"
        );
    }
}
