//! TS-GRP-003, SEC-05: the daemon's panic hook records where the panic
//! happened and nothing of its message. Its own test binary, because the
//! panic hook is process-wide.

use gitraptor_core::daemon::{LOG_FILE, LogLimits, Logger};

#[test]
fn panic_dump_never_contains_the_panic_message() {
    let tmp = tempfile::tempdir().unwrap();
    let logger = Logger::open(tmp.path(), LogLimits::default()).unwrap();
    logger.install_panic_hook();

    let secret = "TOKEN=ghp_PLANTED_SECRET /home/u/repo/.env";
    let result = std::panic::catch_unwind(|| panic!("failed with {secret}"));
    let _ = std::panic::take_hook();
    assert!(result.is_err());

    let log = std::fs::read_to_string(tmp.path().join(LOG_FILE)).unwrap();
    assert!(log.contains(" ERROR panic at="), "{log}");
    assert!(log.contains("daemon_panic_redaction.rs:"), "{log}");
    assert!(!log.contains("PLANTED_SECRET"), "{log}");
    assert!(!log.contains("/home/u"), "{log}");
}
