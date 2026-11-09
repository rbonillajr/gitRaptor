//! DSK002-LINK (TS-DSK-002 T005): the seam between the app and the engine
//! (`gitraptor_desktop::engine::link`), with the shapes of DS-US-DSK-003 § Firmas del stack.
//! `map_client_error` takes every `ClientError` to its `ConnectFailure` (DS-TS-DSK-002
//! § Firmas del stack) and the capabilities the app requires are a fixed list.

use std::io;

use gitraptor_api::client::ClientError;
use gitraptor_api::messages::IncompatibleData;
use gitraptor_api::rpc::ErrorObject;
use gitraptor_desktop::engine::link::{
    ConnectFailure, Incompatibility, IncompatibilityKind, REQUIRED_CAPABILITIES, map_client_error,
};

fn data() -> IncompatibleData {
    IncompatibleData {
        daemon_protocol: 8,
        binary_version: "0.0.8".to_owned(),
    }
}

/// The name of the failure, for the assertion messages (no `Debug` is required of it).
fn name(failure: &ConnectFailure) -> &'static str {
    match failure {
        ConnectFailure::NotRunning => "NotRunning",
        ConnectFailure::LaunchFailed => "LaunchFailed",
        ConnectFailure::Incompatible(_) => "Incompatible",
        ConnectFailure::Rejected => "Rejected",
        ConnectFailure::Unsupported => "Unsupported",
        ConnectFailure::Lost => "Lost",
    }
}

fn assert_maps(err: ClientError, expected: &str) {
    let label = format!("{err:?}");
    let got = map_client_error(err);
    assert_eq!(
        name(&got),
        expected,
        "DSK002-LINK: map_client_error({label}) is not {expected}"
    );
}

fn incompatibility(err: ClientError) -> Incompatibility {
    let label = format!("{err:?}");
    match map_client_error(err) {
        ConnectFailure::Incompatible(inc) => inc,
        other => panic!(
            "DSK002-LINK: map_client_error({label}) is {}, not Incompatible",
            name(&other)
        ),
    }
}

#[test]
fn dsk002_link_not_running_is_not_running() {
    assert_maps(ClientError::NotRunning, "NotRunning");
}

#[test]
fn dsk002_link_an_older_daemon_is_daemon_older_with_its_version() {
    let inc = incompatibility(ClientError::Incompatible(data()));
    assert!(
        matches!(inc.which, IncompatibilityKind::DaemonOlder),
        "DSK002-LINK: Incompatible is not DaemonOlder"
    );
    assert_eq!(inc.daemon_version.as_deref(), Some("0.0.8"));
    assert_eq!(
        inc.daemon_path, None,
        "DSK002-LINK: daemon_path is not None"
    );
    assert!(inc.missing.is_empty(), "DSK002-LINK: missing is not empty");
}

#[test]
fn dsk002_link_a_client_too_old_is_app_older_with_the_daemon_version() {
    let inc = incompatibility(ClientError::ClientTooOld(data()));
    assert!(
        matches!(inc.which, IncompatibilityKind::AppOlder),
        "DSK002-LINK: ClientTooOld is not AppOlder"
    );
    assert_eq!(inc.daemon_version.as_deref(), Some("0.0.8"));
    assert_eq!(
        inc.daemon_path, None,
        "DSK002-LINK: daemon_path is not None"
    );
    assert!(inc.missing.is_empty(), "DSK002-LINK: missing is not empty");
}

#[test]
fn dsk002_link_a_rejected_channel_or_an_unauthentic_peer_is_rejected() {
    assert_maps(ClientError::ChannelRejected, "Rejected");
    assert_maps(ClientError::NotAuthentic, "Rejected");
}

#[test]
fn dsk002_link_an_unsupported_transport_is_unsupported() {
    assert_maps(ClientError::TransportUnsupported, "Unsupported");
}

#[test]
fn dsk002_link_a_failed_launch_or_a_start_timeout_is_launch_failed() {
    assert_maps(
        ClientError::Launch(io::Error::other("spawn failed")),
        "LaunchFailed",
    );
    assert_maps(ClientError::StartTimeout, "LaunchFailed");
}

#[test]
fn dsk002_link_every_other_error_is_lost() {
    assert_maps(
        ClientError::Rpc(ErrorObject::new(-32603, "internal")),
        "Lost",
    );
    assert_maps(ClientError::Io(io::ErrorKind::UnexpectedEof.into()), "Lost");
    assert_maps(ClientError::Protocol("not a contract message"), "Lost");
    assert_maps(ClientError::Unsupported("no runtime folder"), "Lost");
}

#[test]
fn dsk002_link_required_capabilities_are_the_three_of_the_app() {
    assert_eq!(
        REQUIRED_CAPABILITIES.to_vec(),
        vec!["scope.activity", "observation.tiers", "prediction.pairs"],
        "DSK002-LINK: REQUIRED_CAPABILITIES is not the app's list"
    );
}
