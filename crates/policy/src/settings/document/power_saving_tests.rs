//! Criterion test of T004: `engine.powerSaving` is a profile-only key (D15). Asserted through
//! the diagnostics, so it holds whatever the model calls the field.

use super::*;

#[test]
fn power_saving_is_profile_only() {
    let doc = br#"{"engine":{"powerSaving":"on"}}"#;

    let p = parse_document(doc, Level::Profile, SourceKind::Profile);
    assert_eq!(p.status, SourceStatus::Readable);
    assert!(
        p.diagnostics.is_empty(),
        "engine.powerSaving must be accepted at the profile level, got {:?}",
        p.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>()
    );
    for value in ["auto", "on", "off"] {
        let d = format!(r#"{{"engine":{{"powerSaving":"{value}"}}}}"#);
        let p = parse_document(d.as_bytes(), Level::Profile, SourceKind::Profile);
        assert!(
            p.diagnostics.is_empty(),
            "powerSaving {value} is a valid value"
        );
    }

    for (level, kind) in [
        (Level::Team, SourceKind::Team),
        (Level::Local, SourceKind::Local),
    ] {
        let p = parse_document(doc, level, kind);
        let codes: Vec<_> = p.diagnostics.iter().map(|d| d.code.as_str()).collect();
        let at: Vec<_> = p
            .diagnostics
            .iter()
            .map(|d| match &d.location {
                Some(Location::Pointer(s)) => s.clone(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            codes,
            ["key-not-allowed-at-level"],
            "powerSaving outside the profile must be dropped with the level diagnostic"
        );
        assert_eq!(at, ["/engine/powerSaving"]);
    }
}
