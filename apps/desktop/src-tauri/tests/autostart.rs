//! DSK002-FLOW (TS-DSK-002 T007): `autostart::run_choice` asks for the confirmation first and runs
//! `raptor daemon enable|disable` only after a "yes". The confirmation is injected (in production it
//! is the native dialog the webview does not control). The `raptor` is a fake script that records its
//! argv and its environment (`support/fake_raptor.rs`); temporary folders only (NFR-01).
//!
//! Assumed signatures (plan § Firmas que fijan los tests del contrato): `run_choice(&Path, bool, &Offer,
//! Lang, &mut dyn FnMut(&Prompt) -> bool) -> Result<bool, CommandError>`, `prompt(bool, Lang) -> Prompt`.
#![cfg(unix)]

#[path = "support/fake_raptor.rs"]
mod fake_raptor;

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use fake_raptor::FakeRaptor;
use gitraptor_desktop::engine::autostart::{
    Lang, NotOfferedReason, Offer, Prompt, lang_from_apple_languages, prompt, run_choice,
};
use gitraptor_desktop::ipc::CommandError;

/// The fixed `PATH` of the fake-able processes of the app (same as `run_ensure`).
const FIXED_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

fn not_offered() -> Offer {
    Offer::NotOffered {
        reason: NotOfferedReason::Location,
    }
}

/// Runs `run_choice` with a confirmation that answers `answer` and counts how many times it was asked.
fn choose(
    raptor: &Path,
    enable: bool,
    offer: &Offer,
    answer: bool,
) -> (Result<bool, CommandError>, usize) {
    let mut asked = 0;
    let mut confirm = |_: &Prompt| {
        asked += 1;
        answer
    };
    let result = run_choice(raptor, enable, offer, Lang::En, &mut confirm);
    (result, asked)
}

#[test]
fn dsk002_flow_cancel_returns_cancelled_and_runs_nothing() {
    for (enable, offer) in [(true, Offer::Offered), (false, Offer::Registered)] {
        let fake = FakeRaptor::printing("");
        let (result, asked) = choose(&fake.raptor, enable, &offer, false);
        assert_eq!(
            asked, 1,
            "DSK002-FLOW: the confirmation must be asked once (enable = {enable})"
        );
        assert_eq!(
            result,
            Err(CommandError::Cancelled),
            "DSK002-FLOW: a `false` confirmation must be Cancelled (enable = {enable})"
        );
        assert!(
            fake.calls().is_empty(),
            "DSK002-FLOW: `raptor` must not run after a Cancel (enable = {enable}): {:?}",
            fake.calls()
        );
    }
}

#[test]
fn dsk002_flow_yes_runs_daemon_enable_with_only_path_and_returns_registered() {
    let fake = FakeRaptor::printing("");
    let (result, asked) = choose(&fake.raptor, true, &Offer::Offered, true);
    assert_eq!(asked, 1, "DSK002-FLOW: the confirmation must be asked once");
    assert_eq!(
        result,
        Ok(true),
        "DSK002-FLOW: a yes to enable must answer registered = true"
    );
    assert_eq!(
        fake.calls(),
        vec![vec!["daemon".to_owned(), "enable".to_owned()]],
        "DSK002-FLOW: the argv must be exactly `daemon enable`"
    );
    assert_eq!(
        fake.env(),
        BTreeMap::from([("PATH".to_owned(), FIXED_PATH.to_owned())]),
        "DSK002-FLOW: the environment of `raptor` must be only the fixed PATH"
    );
}

#[test]
fn dsk002_flow_yes_runs_daemon_disable_and_returns_not_registered() {
    let fake = FakeRaptor::printing("");
    let (result, _) = choose(&fake.raptor, false, &Offer::Registered, true);
    assert_eq!(
        result,
        Ok(false),
        "DSK002-FLOW: a yes to disable must answer registered = false"
    );
    assert_eq!(
        fake.calls(),
        vec![vec!["daemon".to_owned(), "disable".to_owned()]],
        "DSK002-FLOW: the argv must be exactly `daemon disable`"
    );
    assert_eq!(
        fake.env(),
        BTreeMap::from([("PATH".to_owned(), FIXED_PATH.to_owned())]),
        "DSK002-FLOW: the environment of `disable` must be only the fixed PATH"
    );
}

/// A script that exits with `code`, written like the fake raptor (a `cp` child holds the write
/// descriptor, so no thread of this process does: see `FakeRaptor::write_script`).
fn exiting_with(dir: &Path, code: i32) -> PathBuf {
    let source = dir.join("raptor.sh");
    std::fs::write(&source, format!("#!/bin/sh\nexit {code}\n")).unwrap();
    let raptor = dir.join("raptor");
    let status = std::process::Command::new("/bin/cp")
        .arg(&source)
        .arg(&raptor)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::set_permissions(&raptor, std::fs::Permissions::from_mode(0o755)).unwrap();
    raptor
}

#[test]
fn dsk002_flow_a_non_zero_exit_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let raptor = exiting_with(dir.path(), 3);
    for (enable, offer) in [(true, Offer::Offered), (false, Offer::Registered)] {
        let (result, _) = choose(&raptor, enable, &offer, true);
        assert_eq!(
            result,
            Err(CommandError::Failed),
            "DSK002-FLOW: exit 3 of `raptor` must be Failed (enable = {enable})"
        );
    }
}

#[test]
fn dsk002_flow_a_raptor_that_cannot_run_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let (result, _) = choose(
        &dir.path().join("no-such-raptor"),
        true,
        &Offer::Offered,
        true,
    );
    assert_eq!(
        result,
        Err(CommandError::Failed),
        "DSK002-FLOW: a `raptor` that cannot be executed must be Failed"
    );
}

#[test]
fn dsk002_flow_a_choice_that_does_not_apply_is_not_offered_without_asking() {
    let cases = [
        (true, Offer::Registered),
        (true, not_offered()),
        (false, Offer::Offered),
        (false, not_offered()),
    ];
    for (enable, offer) in cases {
        let fake = FakeRaptor::printing("");
        let (result, asked) = choose(&fake.raptor, enable, &offer, true);
        assert_eq!(
            result,
            Err(CommandError::NotOffered),
            "DSK002-FLOW: enable = {enable} does not apply to this offer: must be NotOffered"
        );
        assert_eq!(
            asked, 0,
            "DSK002-FLOW: the confirmation must not open when the choice does not apply (enable = {enable})"
        );
        assert!(
            fake.calls().is_empty(),
            "DSK002-FLOW: `raptor` must not run when the choice does not apply (enable = {enable})"
        );
    }
}

#[test]
fn dsk002_flow_the_enable_prompt_texts_are_the_ones_of_the_dev_spec() {
    let es = prompt(true, Lang::Es);
    assert_eq!(
        es.title, "Arrancar GitRaptor al iniciar sesión",
        "DSK002-FLOW: es title"
    );
    assert_eq!(
        es.body,
        "GitRaptor arrancará su motor cada vez que inicies sesión. Si no lo pediste tú, cancela.",
        "DSK002-FLOW: es body"
    );
    assert_eq!(es.accept, "Activar", "DSK002-FLOW: es accept");
    assert_eq!(es.cancel, "Cancelar", "DSK002-FLOW: es cancel");

    let en = prompt(true, Lang::En);
    assert_eq!(
        en.title, "Start GitRaptor at login",
        "DSK002-FLOW: en title"
    );
    assert_eq!(
        en.body,
        "GitRaptor will start its engine every time you log in. If you did not ask for this, cancel.",
        "DSK002-FLOW: en body"
    );
    assert_eq!(en.accept, "Enable", "DSK002-FLOW: en accept");
    assert_eq!(en.cancel, "Cancel", "DSK002-FLOW: en cancel");
}

#[test]
fn dsk002_flow_the_disable_prompt_has_its_own_accept_text() {
    let es = prompt(false, Lang::Es);
    assert!(
        es.accept.starts_with("Dejar de arrancar"),
        "DSK002-FLOW: es disable accept starts with `Dejar de arrancar`, got {:?}",
        es.accept
    );
    assert_eq!(es.cancel, "Cancelar", "DSK002-FLOW: es disable cancel");
    let en = prompt(false, Lang::En);
    assert!(
        en.accept.starts_with("Stop starting"),
        "DSK002-FLOW: en disable accept starts with `Stop starting`, got {:?}",
        en.accept
    );
    assert_eq!(en.cancel, "Cancel", "DSK002-FLOW: en disable cancel");
    assert_ne!(
        prompt(false, Lang::En).title,
        prompt(true, Lang::En).title,
        "DSK002-FLOW: disable and enable must not share the title"
    );
}

#[test]
fn dsk002_flow_the_confirmation_receives_the_prompt_of_the_choice() {
    let fake = FakeRaptor::printing("");
    let mut seen: Vec<String> = Vec::new();
    let mut confirm = |p: &Prompt| {
        seen.push(p.title.clone());
        false
    };
    let _ = run_choice(&fake.raptor, true, &Offer::Offered, Lang::Es, &mut confirm);
    assert_eq!(
        seen,
        vec![prompt(true, Lang::Es).title],
        "DSK002-FLOW: the confirmation must get the enable prompt in the language given"
    );
}

/// The output of `defaults read -g AppleLanguages` lists the languages by preference.
fn apple(languages: &[&str]) -> String {
    let items: Vec<String> = languages.iter().map(|l| format!("    \"{l}\"")).collect();
    format!("(\n{}\n)\n", items.join(",\n"))
}

fn is_es(lang: &Lang) -> bool {
    matches!(lang, Lang::Es)
}

#[test]
fn dsk002_flow_a_first_language_starting_with_es_is_spanish() {
    for languages in [
        &["es-ES", "en-US"][..],
        &["es"][..],
        &["es-419", "en"][..],
        &["es-MX"][..],
    ] {
        assert!(
            is_es(&lang_from_apple_languages(&apple(languages))),
            "DSK002-FLOW: {languages:?} must be Es"
        );
    }
}

#[test]
fn dsk002_flow_any_other_first_language_or_output_is_english() {
    for languages in [
        &["en-US", "es-ES"][..],
        &["fr-FR", "es-ES"][..],
        &["de-DE"][..],
        &["en"][..],
    ] {
        assert!(
            !is_es(&lang_from_apple_languages(&apple(languages))),
            "DSK002-FLOW: {languages:?} must be En (only the first language decides)"
        );
    }
    for output in ["", "\n", "(\n)\n", "garbage", "error: domain not found"] {
        assert!(
            !is_es(&lang_from_apple_languages(output)),
            "DSK002-FLOW: the output {output:?} must be En"
        );
    }
}
