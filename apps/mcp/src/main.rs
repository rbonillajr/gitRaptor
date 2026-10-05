mod server;

use std::process::ExitCode;

use rmcp::ServiceExt;
use rmcp::transport::stdio;

/// `raptor-mcp`: the MCP server Claude Code launches over stdio, one per
/// session (ADR-MCP-001 § 1). stdout carries only the protocol; stderr only
/// fixed codes, never data from the repo, the environment or argv (S-10).
/// It never changes directory: its cwd is the caller's scope (§ 2).
fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| eprintln!("raptor-mcp: internal-error")));
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("raptor-mcp: runtime-unavailable");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(async {
        let service = match server::Raptor.serve(stdio()).await {
            Ok(service) => service,
            Err(_) => {
                eprintln!("raptor-mcp: handshake-failed");
                return ExitCode::FAILURE;
            }
        };
        // Ends when Claude Code closes stdin.
        match service.waiting().await {
            Ok(_) => ExitCode::SUCCESS,
            Err(_) => {
                eprintln!("raptor-mcp: transport-failed");
                ExitCode::FAILURE
            }
        }
    })
}
