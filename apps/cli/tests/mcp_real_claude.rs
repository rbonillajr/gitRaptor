//! US-MCP-001 against the real Claude Code CLI (S-MCP-5, R-MCP-6), with a
//! temporary `CLAUDE_CONFIG_DIR` and `HOME`: the developer's configuration
//! is never read nor written (NFR-01). Ignored by default because CI has no
//! Claude Code; run it on a machine that has it:
//!
//! ```sh
//! cargo test -p gitraptor-cli --test mcp_real_claude -- --ignored
//! ```
//!
//! Verified on macOS with Claude Code 2.1.284. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

fn server() -> PathBuf {
    let server = PathBuf::from(RAPTOR).with_file_name("raptor-mcp");
    if !server.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = Command::new(cargo)
            .args(["build", "-q", "-p", "gitraptor-mcp", "--bin", "raptor-mcp"])
            .status()
            .unwrap();
        assert!(status.success());
    }
    server
}

fn real_claude() -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH");
    std::env::split_paths(&path)
        .map(|d| d.join("claude"))
        .find(|c| c.is_file())
        .expect("this test needs the Claude Code CLI on PATH")
}

struct Sandbox {
    tmp: tempfile::TempDir,
    path: String,
}

impl Sandbox {
    fn new() -> Self {
        let claude = real_claude();
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("config")).unwrap();
        fs::create_dir_all(tmp.path().join("home")).unwrap();
        let path = format!("{}:/usr/bin:/bin", claude.parent().unwrap().display());
        Self { tmp, path }
    }

    fn command(&self, exe: &Path, args: &[&str], cwd: &Path) -> Output {
        Command::new(exe)
            .args(args)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", self.tmp.path().join("home"))
            .env("CLAUDE_CONFIG_DIR", self.tmp.path().join("config"))
            .env("GITRAPTOR_PROFILE_DIR", self.tmp.path().join("profile"))
            .env("LANG", "en_US.UTF-8")
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn raptor(&self, args: &[&str]) -> Output {
        self.command(Path::new(RAPTOR), args, self.tmp.path())
    }

    fn claude(&self, args: &[&str]) -> Output {
        self.command(&real_claude(), args, Path::new("/"))
    }

    fn user_servers(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.tmp.path().join("config/.claude.json")).unwrap();
        let config: serde_json::Value = serde_json::from_str(&text).unwrap();
        config["mcpServers"].clone()
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
#[ignore = "needs the Claude Code CLI; run with --ignored"]
fn install_and_uninstall_with_the_real_claude_code() {
    let sb = Sandbox::new();
    let server = server();
    let other = sb.claude(&[
        "mcp",
        "add",
        "--scope",
        "user",
        "--transport",
        "stdio",
        "other",
        "--",
        "/bin/cat",
    ]);
    assert!(other.status.success(), "{}", text(&other));
    let other_before = sb.user_servers()["other"].clone();

    let out = sb.raptor(&["mcp", "install"]);
    assert!(out.status.success(), "{}", text(&out));
    let servers = sb.user_servers();
    assert_eq!(servers["gitraptor"]["type"], "stdio");
    assert_eq!(
        servers["gitraptor"]["command"],
        server.display().to_string()
    );
    assert_eq!(servers["gitraptor"]["args"], serde_json::json!([]));
    assert_eq!(servers["other"], other_before);

    // Claude Code launches it and completes the MCP handshake.
    let get = sb.claude(&["mcp", "get", "gitraptor"]);
    assert!(text(&get).contains("Connected"), "{}", text(&get));

    let again = sb.raptor(&["mcp", "install"]);
    assert!(again.status.success(), "{}", text(&again));
    assert!(text(&again).contains("already installed"));
    assert_eq!(sb.user_servers(), servers);

    let out = sb.raptor(&["mcp", "uninstall"]);
    assert!(out.status.success(), "{}", text(&out));
    let servers = sb.user_servers();
    assert!(servers.get("gitraptor").is_none(), "{servers}");
    assert_eq!(servers["other"], other_before);
}

/// S-MCP-3 repeated with `--scope user` (ADR-MCP-001, Validación 1): Claude
/// Code launches a user-scope stdio server with the cwd where it was run
/// and as its direct parent. The launch is the health check of
/// `claude mcp get`, which needs no login.
#[test]
#[ignore = "needs the Claude Code CLI; run with --ignored"]
fn a_user_scope_server_starts_in_the_session_folder() {
    let sb = Sandbox::new();
    let probe = sb.tmp.path().join("probe.sh");
    let record = sb.tmp.path().join("probe.out");
    fs::write(
        &probe,
        format!(
            "#!/bin/sh\n{{ pwd -P; ps -o comm= -p $PPID; }} > '{}'\n",
            record.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();
    let add = sb.claude(&[
        "mcp",
        "add",
        "--scope",
        "user",
        "--transport",
        "stdio",
        "probe",
        "--",
        probe.to_str().unwrap(),
    ]);
    assert!(add.status.success(), "{}", text(&add));
    let sub = sb.tmp.path().join("proj/sub");
    fs::create_dir_all(&sub).unwrap();
    let _ = sb.command(&real_claude(), &["mcp", "get", "probe"], &sub);
    let seen = fs::read_to_string(&record).unwrap();
    let mut lines = seen.lines();
    assert_eq!(
        lines.next().map(PathBuf::from),
        Some(fs::canonicalize(&sub).unwrap())
    );
    assert!(lines.next().unwrap_or("").ends_with("claude"), "{seen}");
}
