//! `raptor repo roots`, `raptor repo discovered` and `raptor repo dismiss`: the code folders the
//! developer declares and the repos found in them (US-GRP-020, US-GRP-022). Declaring or
//! removing a root and dismissing a candidate are reserved commands (SEC-03): the engine decides
//! who may run them. Accepting a candidate is `raptor repo add`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gitraptor_api::discovery::{
    BroadReason, CandidatesResult, DismissResult, PathParams, RootAddParams, RootAddResult,
    RootBroadData, RootRejectedData, RootRejection, RootRemoveResult, RootsResult,
};
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::{Client, ClientError};

use crate::i18n::t;
use crate::{command_path, confirm, engine, error_text, offers, refusal_text};

/// The engine, when it offers `method`: a daemon older than this binary does not know discovery.
fn engine_with(command: &str, method: &str) -> Result<Client, ExitCode> {
    let client = engine(command)?;
    if offers(&client, method) {
        Ok(client)
    } else {
        eprintln!("{command}: {}", t("error.not-implemented", &[]));
        Err(ExitCode::FAILURE)
    }
}

/// A path as the engine reports it, safe to print.
fn text(raw: &str) -> String {
    sanitize(raw)
}

/// A path ready to paste in a shell: quoted only when it needs it.
fn quoted(raw: &str) -> String {
    let plain = !raw.is_empty()
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+@:=,~".contains(c));
    if plain {
        raw.to_owned()
    } else {
        format!("'{}'", raw.replace('\'', "'\\''"))
    }
}

/// `raptor repo roots`: the declared code folders.
pub(crate) fn roots_list() -> ExitCode {
    const CMD: &str = "raptor repo roots";
    let mut client = match engine_with(CMD, methods::DISCOVERY_ROOTS) {
        Ok(client) => client,
        Err(code) => return code,
    };
    match client.call::<_, RootsResult>(methods::DISCOVERY_ROOTS, serde_json::json!({})) {
        Ok(result) if result.roots.is_empty() => println!("{}", t("discovery.roots-empty", &[])),
        Ok(result) => {
            println!("{}", t("discovery.roots-header", &[]));
            for root in result.roots {
                let key = if root.broad {
                    "discovery.root-line-broad"
                } else {
                    "discovery.root-line"
                };
                println!("{}", t(key, &[("path", &text(&root.path))]));
            }
        }
        Err(err) => return discovery_error(CMD, err),
    }
    ExitCode::SUCCESS
}

/// `raptor repo roots add <path>`. The path is made absolute but never resolved: the engine
/// rejects a symbolic link and says which real path to declare. A broad folder is declared only
/// after the developer confirms its cost on their terminal; without one, nothing is declared.
pub(crate) fn roots_add(path: PathBuf) -> ExitCode {
    const CMD: &str = "raptor repo roots add";
    let path = std::path::absolute(&path).unwrap_or(path);
    let mut client = match engine_with(CMD, methods::DISCOVERY_ROOT_ADD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let mut confirm_broad = false;
    loop {
        let params = RootAddParams {
            path: path.to_string_lossy().into_owned(),
            confirm_broad,
        };
        match client.call::<_, RootAddResult>(methods::DISCOVERY_ROOT_ADD, &params) {
            Ok(result) => {
                print_added(&result);
                return ExitCode::SUCCESS;
            }
            Err(ClientError::Rpc(err))
                if err.code == methods::ROOT_BROAD.code && !confirm_broad =>
            {
                let Some(broad) = err
                    .data
                    .and_then(|d| serde_json::from_value::<RootBroadData>(d).ok())
                else {
                    eprintln!("{CMD}: {}", t("error.root-broad", &[]));
                    return ExitCode::FAILURE;
                };
                let shown = text(&broad.path);
                eprintln!("{CMD}: {}", broad_warning(&broad, &shown));
                if !confirm(&t("discovery.broad-question", &[("path", &shown)])) {
                    eprintln!("{CMD}: {}", t("discovery.broad-declined", &[]));
                    return ExitCode::FAILURE;
                }
                confirm_broad = true;
            }
            Err(ClientError::Rpc(err)) if err.code == methods::ROOT_REJECTED.code => {
                let message = rejected_text(&path, err.data);
                eprintln!("{CMD}: {message}");
                return ExitCode::FAILURE;
            }
            Err(err) => return discovery_error(CMD, err),
        }
    }
}

fn print_added(result: &RootAddResult) {
    let path = text(&result.root.path);
    let key = if result.already {
        "discovery.root-already"
    } else {
        "discovery.root-added"
    };
    println!("{}", t(key, &[("path", &path)]));
    match result.candidates {
        0 => {}
        1 => println!("{}", t("discovery.found-one", &[])),
        count => println!("{}", t("discovery.found", &[("count", &count)])),
    }
    if result.candidates > 0 {
        println!("{}", t("discovery.found-hint", &[]));
    }
}

/// The cost of a broad root, in words (RES-03).
fn broad_warning(broad: &RootBroadData, shown: &str) -> String {
    let why = match broad.reason {
        BroadReason::Home => t("discovery.broad-home", &[]),
        BroadReason::Volume => t("discovery.broad-volume", &[]),
        BroadReason::Entries => t("discovery.broad-entries", &[("entries", &broad.entries)]),
    };
    t(
        "discovery.broad-warning",
        &[("path", &shown), ("why", &why)],
    )
}

/// What was wrong with the path and what to do instead.
fn rejected_text(path: &Path, data: Option<serde_json::Value>) -> String {
    let data = data.and_then(|d| serde_json::from_value::<RootRejectedData>(d).ok());
    let shown = text(&path.to_string_lossy());
    let Some(data) = data else {
        return t("error.root-rejected", &[]);
    };
    let args = |real: &str| [("path", shown.clone()), ("real", real.to_owned())];
    let key = match data.reason {
        RootRejection::NotAbsolute => "discovery.rejected-not-absolute",
        RootRejection::Missing => "discovery.rejected-missing",
        RootRejection::NotADirectory => "discovery.rejected-not-a-directory",
        RootRejection::Symlink if data.real_path.is_some() => "discovery.rejected-symlink",
        RootRejection::Symlink => "discovery.rejected-symlink-plain",
        RootRejection::FilesystemRoot => "discovery.rejected-filesystem-root",
        RootRejection::HomeAncestor => "discovery.rejected-home-ancestor",
        RootRejection::InsideRepo => "discovery.rejected-inside-repo",
        RootRejection::Profile => "discovery.rejected-profile",
        RootRejection::Network => "discovery.rejected-network",
        RootRejection::TooMany => "discovery.rejected-too-many",
        RootRejection::Unreadable => "discovery.rejected-unreadable",
    };
    let real = data
        .real_path
        .as_deref()
        .map(|p| quoted(&text(p)))
        .unwrap_or_default();
    let [(n0, v0), (n1, v1)] = args(&real);
    t(key, &[(n0, &v0), (n1, &v1)])
}

/// `raptor repo roots remove <path>`: its pending candidates go; observed repos stay.
pub(crate) fn roots_remove(path: PathBuf) -> ExitCode {
    const CMD: &str = "raptor repo roots remove";
    let path = command_path(Some(path));
    let mut client = match engine_with(CMD, methods::DISCOVERY_ROOT_REMOVE) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let params = PathParams {
        path: path.to_string_lossy().into_owned(),
    };
    match client.call::<_, RootRemoveResult>(methods::DISCOVERY_ROOT_REMOVE, &params) {
        Ok(result) => {
            println!(
                "{}",
                t(
                    "discovery.root-removed",
                    &[
                        ("path", &text(&result.root)),
                        ("count", &result.candidates_removed)
                    ]
                )
            );
            ExitCode::SUCCESS
        }
        Err(err) => discovery_error(CMD, err),
    }
}

/// `raptor repo discovered`: the repos found and not yet decided, with how to decide.
pub(crate) fn discovered() -> ExitCode {
    const CMD: &str = "raptor repo discovered";
    let mut client = match engine_with(CMD, methods::DISCOVERY_CANDIDATES) {
        Ok(client) => client,
        Err(code) => return code,
    };
    match client.call::<_, CandidatesResult>(methods::DISCOVERY_CANDIDATES, serde_json::json!({})) {
        Ok(result) if result.candidates.is_empty() => println!("{}", t("discovery.none", &[])),
        Ok(result) => {
            for candidate in result.candidates {
                let name = text(&candidate.name);
                let path = text(&candidate.path);
                println!(
                    "{}",
                    t(
                        "discovery.candidate-line",
                        &[
                            ("name", &name),
                            ("path", &path),
                            ("root", &text(&candidate.root))
                        ]
                    )
                );
                println!("  {}", t("discovery.ask", &[("name", &name)]));
                let path = quoted(&path);
                println!("    {}", t("discovery.cmd-add", &[("path", &path)]));
                println!("    {}", t("discovery.cmd-dismiss", &[("path", &path)]));
            }
        }
        Err(err) => return discovery_error(CMD, err),
    }
    ExitCode::SUCCESS
}

/// `raptor repo dismiss <path>`: never proposed again; `raptor repo add` still observes it.
pub(crate) fn dismiss(path: PathBuf) -> ExitCode {
    const CMD: &str = "raptor repo dismiss";
    let path = command_path(Some(path));
    let mut client = match engine_with(CMD, methods::DISCOVERY_DISMISS) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let params = PathParams {
        path: path.to_string_lossy().into_owned(),
    };
    match client.call::<_, DismissResult>(methods::DISCOVERY_DISMISS, &params) {
        Ok(result) => {
            println!(
                "{}",
                t("discovery.dismissed", &[("path", &text(&result.path))])
            );
            ExitCode::SUCCESS
        }
        Err(err) => discovery_error(CMD, err),
    }
}

/// The message for a failed call: an agent's refusal says what only the developer may do; any
/// other code is presented from the code, never from the engine's message (N7).
fn discovery_error(command: &str, err: ClientError) -> ExitCode {
    let message = match err {
        ClientError::Rpc(err) if err.code == code::RESERVED_REFUSED => {
            refusal_text(&err, "discovery.refused-agent")
        }
        other => error_text(other),
    };
    eprintln!("{command}: {message}");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n;

    /// Every rejection and every broad reason has its message in both languages.
    #[test]
    fn every_reason_has_a_message() {
        for key in [
            "discovery.rejected-not-absolute",
            "discovery.rejected-missing",
            "discovery.rejected-not-a-directory",
            "discovery.rejected-symlink",
            "discovery.rejected-symlink-plain",
            "discovery.rejected-filesystem-root",
            "discovery.rejected-home-ancestor",
            "discovery.rejected-inside-repo",
            "discovery.rejected-profile",
            "discovery.rejected-network",
            "discovery.rejected-too-many",
            "discovery.rejected-unreadable",
            "discovery.broad-home",
            "discovery.broad-volume",
            "discovery.broad-entries",
        ] {
            assert!(i18n::has_key(key), "{key}");
        }
    }

    #[test]
    fn a_symlink_suggests_its_real_path() {
        let data = serde_json::to_value(RootRejectedData {
            reason: RootRejection::Symlink,
            real_path: Some("/real/code".into()),
        })
        .unwrap();
        let message = rejected_text(Path::new("/link"), Some(data));
        assert!(message.contains("/real/code"), "{message}");
    }

    #[test]
    fn paths_are_quoted_only_when_needed() {
        assert_eq!(quoted("/a/b-c_d.e"), "/a/b-c_d.e");
        assert_eq!(quoted("/a b/it's"), "'/a b/it'\\''s'");
    }
}
