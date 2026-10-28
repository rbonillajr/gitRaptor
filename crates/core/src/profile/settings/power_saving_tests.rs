//! Criterion test of T004: `power_saving(dirs)` reads `engine.powerSaving` of the profile.

use gitraptor_api::resources::PowerSavingSetting;

use super::*;

#[test]
fn power_saving_defaults_to_auto() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path());
    std::fs::create_dir_all(&dirs.config).unwrap();
    let write = |text: &str| std::fs::write(dirs.config.join(PROFILE_SETTINGS_FILE), text).unwrap();

    assert_eq!(
        power_saving(&dirs),
        PowerSavingSetting::Auto,
        "no settings file means auto"
    );
    write(r#"{"engine":{}}"#);
    assert_eq!(
        power_saving(&dirs),
        PowerSavingSetting::Auto,
        "no key means auto"
    );
    write("{not json");
    assert_eq!(
        power_saving(&dirs),
        PowerSavingSetting::Auto,
        "an unusable document means auto"
    );
    write(r#"{"engine":{"powerSaving":"off"}}"#);
    assert_eq!(
        power_saving(&dirs),
        PowerSavingSetting::Off,
        "the profile value wins"
    );
    write(r#"{"engine":{"powerSaving":"on"}}"#);
    assert_eq!(power_saving(&dirs), PowerSavingSetting::On);
}
