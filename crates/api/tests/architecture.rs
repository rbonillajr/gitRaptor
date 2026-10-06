//! Architecture test (ADR-GRP-016): nothing new is declared in a shared
//! file. A method, a notification, a capability or an error code goes in
//! its module's file under `src/methods/`; these files only keep what froze
//! when the registry was split, and this test fails if that changes.

const METHODS_MOD: &str = include_str!("../src/methods/mod.rs");
const RPC: &str = include_str!("../src/rpc.rs");
const LIB: &str = include_str!("../src/lib.rs");

/// `pub const NAME: <ty>` declarations of a source text.
fn consts<'a>(source: &'a str, ty: &str) -> Vec<&'a str> {
    source
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("pub const "))
        .filter_map(|l| l.split_once(':'))
        .filter(|(name, rest)| {
            name.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && rest.trim_start().starts_with(ty)
        })
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn the_method_registry_declares_no_method() {
    assert_eq!(
        consts(METHODS_MOD, "&str"),
        Vec::<&str>::new(),
        "a method or notification name in methods/mod.rs: declare it in its module's file"
    );
    assert!(
        !METHODS_MOD.contains("Capability::new(") && !METHODS_MOD.contains("Capability::legacy("),
        "a capability in methods/mod.rs: declare it in its module's file"
    );
    assert!(
        !METHODS_MOD.contains("ErrorSpec::new(") && !METHODS_MOD.contains("ErrorSpec {"),
        "an error code in methods/mod.rs: declare it in its module's block"
    );
}

/// The shared list of codes froze at `-32016` with these 21 codes.
#[test]
fn the_shared_error_list_is_frozen() {
    let frozen = [
        "PARSE_ERROR",
        "INVALID_REQUEST",
        "METHOD_NOT_FOUND",
        "INVALID_PARAMS",
        "INTERNAL",
        "HANDSHAKE_REQUIRED",
        "INCOMPATIBLE_PROTOCOL",
        "RESERVED_REFUSED",
        "NOT_IMPLEMENTED",
        "RATE_LIMITED",
        "LIMIT_REACHED",
        "RESYNC_REQUIRED",
        "PRIOR_SNAPSHOT_FAILED",
        "NOT_FOUND",
        "SCOPE_REFUSED",
        "OPERATION_FAILED",
        "IDENTITY_UNVERIFIED",
        "REPO_REJECTED",
        "OPERATION_REJECTED",
        "REGISTRATION_REJECTED",
        "GUARD_REJECTED",
    ];
    assert_eq!(
        consts(RPC, "i64"),
        frozen,
        "a new code in rpc.rs: declare it as an ErrorSpec in its module's block"
    );
    assert_eq!(gitraptor_api::rpc::ErrorCode::ALL.len(), frozen.len());
    assert!(
        !RPC.contains("ErrorSpec::new(-"),
        "an error code declared in rpc.rs: declare it in its module's block"
    );
}

/// The protocol number froze at 9 for additive changes: a feature is a
/// method or a capability of its module. Only removing something bumps it,
/// and that is a decision to record in an ADR, not a story's edit.
#[test]
fn the_protocol_number_is_frozen() {
    assert_eq!(gitraptor_api::PROTOCOL_VERSION, 9);
    assert_eq!(gitraptor_api::API_VERSION, "9.0.0");
    assert_eq!(gitraptor_api::MIN_COMPATIBLE_PROTOCOL, 5);
    assert_eq!(
        consts(LIB, "u32").len(),
        2,
        "a new protocol constant in lib.rs"
    );
}

/// Every method added from protocol 9 on is visible at 9: a later number
/// would bring the counter back.
#[test]
fn no_method_waits_for_a_newer_protocol() {
    for m in gitraptor_api::methods::METHODS.iter() {
        assert!(m.since <= gitraptor_api::PROTOCOL_VERSION, "{}", m.name);
    }
}
