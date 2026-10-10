mod engine;
mod messages;
mod server;
mod snapshot;
mod status;
mod stdout;

use std::process::ExitCode;

use rmcp::ServiceExt;

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
        // The writes of the transport never wait (see `stdout`): an answer cannot be cut half way.
        let (writer, drain) = stdout::queued(tokio::io::stdout());
        let service =
            match server::Raptor::new(messages::Lang::from_env(|key| std::env::var(key).ok()))
                .serve((tokio::io::stdin(), writer))
                .await
            {
                Ok(service) => service,
                Err(_) => {
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
