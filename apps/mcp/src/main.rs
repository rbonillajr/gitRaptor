use std::process::ExitCode;

use gitraptor_api::messages::ClientKind;
use gitraptor_core::client::{ClientOptions, ensure_daemon};
use gitraptor_core::profile::ProfileDirs;

/// `raptor-mcp`. The MCP tools are F-001-05; this TS only connects to the
/// engine, starting it on demand with a clean environment (TS-GRP-004).
fn main() -> ExitCode {
    let dirs = match ProfileDirs::resolve() {
        Ok(dirs) => dirs,
        Err(err) => {
            eprintln!("raptor-mcp: {err}");
            return ExitCode::FAILURE;
        }
    };
    match ensure_daemon(&ClientOptions::new(dirs, ClientKind::Mcp)) {
        Ok(client) => {
            eprintln!(
                "raptor-mcp {}: connected to the engine (protocol {}); MCP tools are not implemented yet",
                gitraptor_core::version(),
                client.hello().protocol
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("raptor-mcp: {err}");
            ExitCode::FAILURE
        }
    }
}
