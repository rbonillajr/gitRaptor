mod engine;
mod input;
mod messages;
mod server;
mod snapshot;
mod status;
mod stdout;

use std::process::ExitCode;

use rmcp::ServiceExt;
use tokio::io::{BufReader, duplex};
use tokio::sync::Mutex;

/// Pipe capacity between the bounded stdin pump and rmcp.
const PIPE_BYTES: usize = 64 * 1024;

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
    let code = runtime.block_on(async {
        // rmcp never reads stdin directly: `input` enforces the message cap of ADR-MCP-001 § 6
        // before rmcp sees a byte. Every byte that leaves goes through one queue (see `stdout`):
        // rmcp's answers and the refusals of the cap share it, so a line is never cut half way
        // or interleaved with another, and a write never waits.
        let (writer, drain) = stdout::queued(tokio::io::stdout());
        let refusals = Mutex::new(writer.clone());
        let (pump_in, server_in) = duplex(PIPE_BYTES);
        tokio::spawn(async move {
            let _ = input::pump_input(BufReader::new(tokio::io::stdin()), pump_in, &refusals).await;
        });
        let service =
            match server::Raptor::new(messages::Lang::from_env(|key| std::env::var(key).ok()))
                .serve((server_in, writer))
                .await
            {
                Ok(service) => service,
                Err(_) => {
                    // rmcp may have answered a malformed `initialize`.
                    drain.finish(std::time::Duration::from_secs(1)).await;
                    eprintln!("raptor-mcp: handshake-failed");
                    // The transport is gone with `serve`, and the error answer is still queued.
                    drain.finish(std::time::Duration::from_secs(1)).await;
                    return ExitCode::FAILURE;
                }
            };
        // Ends when Claude Code closes stdin.
        let code = match service.waiting().await {
            Ok(_) => ExitCode::SUCCESS,
            Err(_) => {
                eprintln!("raptor-mcp: transport-failed");
                ExitCode::FAILURE
            }
        };
        // What is queued still goes out; a reader that stopped reading does not keep us here.
        drain.finish(std::time::Duration::from_secs(1)).await;
        code
    });
    // A call still running past its time limit must not keep the process
    // alive once Claude Code is gone.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    code
}
