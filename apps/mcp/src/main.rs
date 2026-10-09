mod engine;
mod input;
mod messages;
mod server;
mod snapshot;
mod status;

use std::process::ExitCode;
use std::sync::Arc;

use rmcp::ServiceExt;
use tokio::io::{BufReader, duplex};
use tokio::sync::Mutex;

/// Pipe capacity between the bounded stdin/stdout pumps and rmcp.
const PIPE_BYTES: usize = 64 * 1024;

/// Waits (bounded: a client that stopped reading must not keep the process
/// alive) for the server's last replies to reach stdout.
async fn drain(output: tokio::task::JoinHandle<()>) {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), output).await;
}

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
        // rmcp never reads stdin directly: `input` enforces the message cap of
        // ADR-MCP-001 § 6 before rmcp sees a byte, and one lock keeps whole
        // lines on stdout.
        let stdout = Arc::new(Mutex::new(tokio::io::stdout()));
        let (pump_in, server_in) = duplex(PIPE_BYTES);
        let (server_out, pump_out) = duplex(PIPE_BYTES);
        tokio::spawn({
            let stdout = Arc::clone(&stdout);
            async move {
                let _ =
                    input::pump_input(BufReader::new(tokio::io::stdin()), pump_in, &stdout).await;
            }
        });
        let output = tokio::spawn({
            let stdout = Arc::clone(&stdout);
            async move {
                let _ = input::pump_output(BufReader::new(pump_out), &stdout).await;
            }
        });
        let service =
            match server::Raptor::new(messages::Lang::from_env(|key| std::env::var(key).ok()))
                .serve((server_in, server_out))
                .await
            {
                Ok(service) => service,
                Err(_) => {
                    // rmcp may have answered a malformed `initialize`.
                    drain(output).await;
                    eprintln!("raptor-mcp: handshake-failed");
                    return ExitCode::FAILURE;
                }
            };
        // Ends when Claude Code closes stdin.
        let ended = service.waiting().await;
        // The server's last replies must reach stdout before the process ends.
        drain(output).await;
        match ended {
            Ok(_) => ExitCode::SUCCESS,
            Err(_) => {
                eprintln!("raptor-mcp: transport-failed");
                ExitCode::FAILURE
            }
        }
    });
    // A call still running past its time limit must not keep the process
    // alive once Claude Code is gone.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    code
}
