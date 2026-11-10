//! DSK002-OFFER (TS-DSK-002 T007): `autostart::offer_with_home` decides whether the app offers to
//! start the engine at login. `Offered` only from `/Applications/` or `<home>/Applications/`, never
//! from `/Volumes/` or an `/AppTranslocation/` path, and only when the autostart is not registered;
//! registered with the bundle's `raptor` is `Registered`; registered with another binary is
//! `NotOffered { OtherBinary }`. Pure rule: the paths are strings, nothing touches the disk.
//!
//! Assumed signatures (plan § Firmas que fijan los tests del contrato):
//! `offer_with_home(&Path, &Path, &EnsureReport) -> Offer`, `Offer`, `NotOfferedReason`.

use std::path::Path;

use gitraptor_api::client::ensure::EnsureReport;
use gitraptor_desktop::engine::autostart::{NotOfferedReason, Offer, offer_with_home};
use serde_json::{Value, json};

const HOME: &str = "/Users/ana";
const APP: &str = "/Applications/GitRaptor.app/Contents/MacOS/gitraptor-desktop";
const BUNDLE_RAPTOR: &str = "/Applications/GitRaptor.app/Contents/MacOS/raptor";

/// An `ensure` report of the shared fixture `ready.json`, with the `autostart` block replaced.
fn report(registered: bool, binary: Option<&str>) -> EnsureReport {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../crates/api/tests/fixtures/ensure/ready.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()));
    let mut value: Value = serde_json::from_str(&text).unwrap();
    value["autostart"] = json!({ "registered": registered, "binary": binary });
    serde_json::from_value(value).expect("the fixture with another autostart is an EnsureReport")
}

fn offer(app: &str, home: &str, report: &EnsureReport) -> Offer {
    offer_with_home(Path::new(app), Path::new(home), report)
}

fn is_offered(o: &Offer) -> bool {
    matches!(o, Offer::Offered)
}

fn is_registered(o: &Offer) -> bool {
    matches!(o, Offer::Registered)
}

fn is_location(o: &Offer) -> bool {
    matches!(
        o,
        Offer::NotOffered {
            reason: NotOfferedReason::Location
        }
    )
}

fn is_other_binary(o: &Offer) -> bool {
    matches!(
        o,
        Offer::NotOffered {
            reason: NotOfferedReason::OtherBinary
        }
    )
}

#[test]
fn dsk002_offer_system_applications_is_offered() {
    assert!(
        is_offered(&offer(APP, HOME, &report(false, None))),
        "DSK002-OFFER: /Applications/GitRaptor.app/... with no autostart must be Offered"
    );
}

#[test]
fn dsk002_offer_user_applications_is_offered() {
    let app = "/Users/ana/Applications/GitRaptor.app/Contents/MacOS/gitraptor-desktop";
    assert!(
        is_offered(&offer(app, HOME, &report(false, None))),
        "DSK002-OFFER: <home>/Applications/GitRaptor.app/... must be Offered"
    );
}

#[test]
fn dsk002_offer_another_users_applications_is_a_location() {
    let app = "/Users/bob/Applications/GitRaptor.app/Contents/MacOS/gitraptor-desktop";
    assert!(
        is_location(&offer(app, HOME, &report(false, None))),
        "DSK002-OFFER: the Applications folder of another user is not <home>/Applications: NotOffered Location"
    );
}

#[test]
fn dsk002_offer_a_mounted_volume_is_a_location() {
    let app = "/Volumes/GitRaptor/GitRaptor.app/Contents/MacOS/gitraptor-desktop";
    assert!(
        is_location(&offer(app, HOME, &report(false, None))),
        "DSK002-OFFER: /Volumes/... must be NotOffered Location"
    );
}

#[test]
fn dsk002_offer_a_translocated_path_is_a_location() {
    let app = "/private/var/folders/ab/xyz/AppTranslocation/0F3E/d/GitRaptor.app/Contents/MacOS/gitraptor-desktop";
    assert!(
        is_location(&offer(app, HOME, &report(false, None))),
        "DSK002-OFFER: a path with /AppTranslocation/ must be NotOffered Location"
    );
    let inside = "/Applications/AppTranslocation/GitRaptor.app/Contents/MacOS/gitraptor-desktop";
    assert!(
        is_location(&offer(inside, HOME, &report(false, None))),
        "DSK002-OFFER: /AppTranslocation/ wins even under /Applications/"
    );
}

#[test]
fn dsk002_offer_a_path_outside_applications_is_a_location() {
    for app in [
        "/Users/ana/Downloads/GitRaptor.app/Contents/MacOS/gitraptor-desktop",
        "/opt/GitRaptor.app/Contents/MacOS/gitraptor-desktop",
        "/ApplicationsX/GitRaptor.app/Contents/MacOS/gitraptor-desktop",
    ] {
        assert!(
            is_location(&offer(app, HOME, &report(false, None))),
            "DSK002-OFFER: {app} is outside /Applications: NotOffered Location"
        );
    }
}

#[test]
fn dsk002_offer_registered_with_the_bundle_raptor_is_registered() {
    assert!(
        is_registered(&offer(APP, HOME, &report(true, Some(BUNDLE_RAPTOR)))),
        "DSK002-OFFER: autostart registered with the raptor of the bundle must be Registered"
    );
}

#[test]
fn dsk002_offer_registered_with_another_binary_is_not_offered() {
    assert!(
        is_other_binary(&offer(
            APP,
            HOME,
            &report(true, Some("/opt/homebrew/bin/raptor"))
        )),
        "DSK002-OFFER: autostart registered with another raptor must be NotOffered OtherBinary"
    );
}
