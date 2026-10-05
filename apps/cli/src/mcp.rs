//! `raptor mcp install` and `raptor mcp uninstall` (US-MCP-001,
//! ADR-MCP-001 § 8, BR-MCP-WF-007, BR-MCP-EDGE-009).
//!
//! The server is registered through the Claude Code CLI, never by editing
//! its configuration: `claude` is run by absolute path with a fixed argv
//! and no shell (NFR-02), from `/` so that `claude mcp get` does not pick
//! up a project `.mcp.json` or the local scope of the current folder. The
//! form of that CLI is an assumption (S-MCP-5): an answer of another shape
//! changes nothing and says so.
//!
//! Neither command talks to the engine, touches a repo or the MCP
//! allowlist (US-MCP-002 owns that).

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use gitraptor_api::untrusted::sanitize;

use crate::i18n::t;

/// The name of the server in Claude Code.
pub const SERVER_NAME: &str = "gitraptor";

const CLAUDE: &str = if cfg!(windows) {
    "claude.exe"
} else {
    "claude"
};
const SERVER_BIN: &str = if cfg!(windows) {
    "raptor-mcp.exe"
} else {
    "raptor-mcp"
};

/// The agent a command targets. Only Claude Code in the MVP (D2).
#[derive(Debug, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
    /// Its name as shown to the developer.
    Unsupported(String),
}

impl Agent {
    pub fn parse(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "claude-code" | "claude" => Self::ClaudeCode,
            "cursor" => Self::Unsupported("Cursor".into()),
            "codex" => Self::Unsupported("Codex".into()),
            "copilot" => Self::Unsupported("Copilot".into()),
            _ => Self::Unsupported(sanitize(name)),
        }
    }
}

/// What `claude mcp get gitraptor` says.
#[derive(Debug, PartialEq, Eq)]
pub enum Registration {
    Missing,
    /// In the user scope, over stdio, with its command, args and env.
    User {
        stdio: bool,
        command: String,
        args: String,
        env: Vec<String>,
    },
    /// Registered in another scope (project, local).
    OtherScope,
    /// An answer this version does not understand (S-MCP-5).
    Unknown,
}

impl Registration {
    /// Parses the output of `claude mcp get` (Claude Code 2.1.284; the
    /// fixtures in `tests/fixtures` pin the shape).
    pub fn parse(success: bool, stdout: &str, stderr: &str) -> Self {
        if !success {
            return if stderr.contains("No MCP server named")
                || stdout.contains("No MCP server named")
            {
                Self::Missing
            } else {
                Self::Unknown
            };
        }
        let mut lines = stdout.lines();
        if lines.next().map(str::trim_end) != Some(&format!("{SERVER_NAME}:")) {
            return Self::Unknown;
        }
        let (mut scope, mut kind, mut command, mut args) = (None, None, None, None);
        let mut env = Vec::new();
        let mut in_env = false;
        for line in lines {
            let trimmed = line.trim();
            if in_env {
                if line.starts_with("    ") && !trimmed.is_empty() {
                    env.push(trimmed.to_owned());
                    continue;
                }
                in_env = false;
            }
            if let Some(v) = trimmed.strip_prefix("Scope:") {
                scope = Some(v.trim().to_owned());
            } else if let Some(v) = trimmed.strip_prefix("Type:") {
                kind = Some(v.trim().to_owned());
            } else if let Some(v) = trimmed.strip_prefix("Command:") {
                command = Some(v.trim().to_owned());
            } else if let Some(v) = trimmed.strip_prefix("Args:") {
                args = Some(v.trim().to_owned());
            } else if trimmed == "Environment:" {
                in_env = true;
            }
        }
        match scope {
            None => Self::Unknown,
            Some(scope) if !scope.starts_with("User config") => Self::OtherScope,
            Some(_) => match (kind, command, args) {
                (Some(kind), Some(command), Some(args)) => Self::User {
                    stdio: kind == "stdio",
                    command,
                    args,
                    env,
                },
                _ => Self::Unknown,
            },
        }
    }

    /// This GitRaptor's server: user scope, stdio, our binary (compared
    /// canonically on both sides), and nothing else (BR-MCP-EDGE-009).
    pub fn is_ours(&self, server: &Path) -> bool {
        match self {
            Self::User {
                stdio,
                command,
                args,
                env,
            } => {
                *stdio && args.is_empty() && env.is_empty() && same_file(Path::new(command), server)
            }
            _ => false,
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `raptor-mcp` next to the running `raptor`, absolute but not
/// canonicalized: a Homebrew symlink stays valid across upgrades.
fn server_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = std::path::absolute(exe).ok()?;
    Some(exe.with_file_name(SERVER_BIN))
}

/// The refusal key when the binary is not an installed GitRaptor (SEC-14).
pub fn check_installed(server: &Path) -> Result<(), &'static str> {
    let canonical = std::fs::canonicalize(server).ok();
    let mut paths = vec![server.to_path_buf()];
    if let Some(dir) = server.parent() {
        paths.push(dir.to_path_buf());
    }
    paths.extend(canonical.clone());
    if paths.iter().any(|p| is_volatile(p)) {
        return Err("mcp.not-installed");
    }
    let Some(canonical) = canonical else {
        return Err("mcp.server-missing");
    };
    let Ok(meta) = std::fs::metadata(&canonical) else {
        return Err("mcp.server-missing");
    };
    if !meta.is_file() {
        return Err("mcp.server-missing");
    }
    if writable_by_others(&meta) {
        return Err("mcp.server-unsafe");
    }
    Ok(())
}

/// In an npx, pnpm dlx or bunx cache, or under a temporary folder. `TMPDIR`
/// can be set by anyone: at worst it makes a false refusal.
pub fn is_volatile(path: &Path) -> bool {
    let names: Vec<&OsStr> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    let cache = names.iter().enumerate().any(|(i, name)| {
        let name = name.to_string_lossy();
        name == "_npx"
            || name.starts_with("bunx-")
            || (name == "dlx" && i > 0 && names[i - 1].to_string_lossy().contains("pnpm"))
    });
    if cache {
        return true;
    }
    let mut temp_roots: Vec<PathBuf> = vec![std::env::temp_dir()];
    if cfg!(unix) {
        temp_roots.extend(
            [
                "/tmp",
                "/private/tmp",
                "/var/tmp",
                "/var/folders",
                "/private/var/folders",
            ]
            .map(PathBuf::from),
        );
    }
    let extra: Vec<PathBuf> = temp_roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .collect();
    temp_roots.extend(extra);
    temp_roots
        .iter()
        .filter(|root| root.parent().is_some())
        .any(|root| path.starts_with(root))
}

#[cfg(unix)]
fn writable_by_others(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o022 != 0
}

#[cfg(not(unix))]
fn writable_by_others(_meta: &std::fs::Metadata) -> bool {
    // Pendiente: etapa de validación multiplataforma (ACLs on Windows).
    false
}

/// The Claude Code CLI on the user's `PATH`: absolute folders only, a
/// regular file that neither the group nor others can modify.
fn find_claude() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(CLAUDE))
        .find_map(|candidate| {
            let canonical = std::fs::canonicalize(&candidate).ok()?;
            let meta = std::fs::metadata(&canonical).ok()?;
            (meta.is_file() && !writable_by_others(&meta)).then_some(canonical)
        })
}

fn add_args(server: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "mcp",
        "add",
        "--scope",
        "user",
        "--transport",
        "stdio",
        SERVER_NAME,
        "--",
    ]
    .map(OsString::from)
    .to_vec();
    args.push(server.as_os_str().to_owned());
    args
}

fn remove_args() -> Vec<OsString> {
    ["mcp", "remove", "--scope", "user", SERVER_NAME]
        .map(OsString::from)
        .to_vec()
}

/// The command as the developer would type it, quoted for a POSIX shell.
pub fn shown_command(args: &[OsString]) -> String {
    let mut out = String::from("claude");
    for arg in args {
        let arg = arg.to_string_lossy();
        out.push(' ');
        if !arg.is_empty()
            && arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/._-+:@,=".contains(c))
        {
            out.push_str(&arg);
        } else {
            out.push('\'');
            out.push_str(&arg.replace('\'', r"'\''"));
            out.push('\'');
        }
    }
    sanitize(&out)
}

struct Output {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs `claude` with a fixed argv, no shell and no stdin, from `/`.
fn run_claude(claude: &Path, args: &[OsString]) -> Option<Output> {
    let root = if cfg!(windows) { "C:\\" } else { "/" };
    let output = Command::new(claude)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    Some(Output {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn registration(claude: &Path) -> Registration {
    match run_claude(claude, &["mcp", "get", SERVER_NAME].map(OsString::from)) {
        Some(out) => Registration::parse(out.success, &out.stdout, &out.stderr),
        None => Registration::Unknown,
    }
}

fn refuse_agent(command: &str, agent: &Agent) -> Option<ExitCode> {
    match agent {
        Agent::ClaudeCode => None,
        Agent::Unsupported(name) => {
            eprintln!(
                "{command}: {}",
                t("mcp.agent-unsupported", &[("agent", name)])
            );
            Some(ExitCode::FAILURE)
        }
    }
}

fn claude_failed(command: &str, out: &Output) -> ExitCode {
    let code = out.code.map_or_else(|| "-".to_owned(), |c| c.to_string());
    eprintln!("{command}: {}", t("mcp.claude-failed", &[("code", &code)]));
    if let Some(line) = out.stderr.lines().find(|l| !l.trim().is_empty()) {
        eprintln!("  {}", sanitize(line));
    }
    ExitCode::FAILURE
}

/// `raptor mcp install`.
pub fn install(agent: &str) -> ExitCode {
    const CMD: &str = "raptor mcp install";
    if let Some(code) = refuse_agent(CMD, &Agent::parse(agent)) {
        return code;
    }
    let Some(server) = server_path() else {
        eprintln!(
            "{CMD}: {}",
            t("mcp.server-missing", &[("path", &"raptor-mcp")])
        );
        return ExitCode::FAILURE;
    };
    let shown_server = sanitize(&server.display().to_string());
    if let Err(key) = check_installed(&server) {
        eprintln!("{CMD}: {}", t(key, &[("path", &shown_server)]));
        return ExitCode::FAILURE;
    }
    let args = add_args(&server);
    let Some(claude) = find_claude() else {
        eprintln!("{CMD}: {}", t("mcp.claude-missing", &[]));
        println!("{}", shown_command(&args));
        return ExitCode::FAILURE;
    };
    match registration(&claude) {
        Registration::Missing => {}
        found if found.is_ours(&server) => {
            println!("{}", t("mcp.install.already", &[]));
            return ExitCode::SUCCESS;
        }
        Registration::Unknown => {
            eprintln!("{CMD}: {}", t("mcp.claude-changed", &[]));
            println!("{}", shown_command(&args));
            return ExitCode::FAILURE;
        }
        Registration::User { .. } | Registration::OtherScope => {
            eprintln!("{CMD}: {}", t("mcp.foreign", &[]));
            return ExitCode::FAILURE;
        }
    }
    println!("{}", t("mcp.install.plan", &[]));
    println!("  {}", shown_command(&args));
    match run_claude(&claude, &args) {
        Some(out) if out.success => {
            println!("{}", t("mcp.install.done", &[("path", &shown_server)]));
            ExitCode::SUCCESS
        }
        Some(out) => claude_failed(CMD, &out),
        None => {
            eprintln!("{CMD}: {}", t("mcp.claude-missing", &[]));
            println!("{}", shown_command(&args));
            ExitCode::FAILURE
        }
    }
}

/// `raptor mcp uninstall`: removes "gitraptor" only when it is ours.
pub fn uninstall(agent: &str) -> ExitCode {
    const CMD: &str = "raptor mcp uninstall";
    if let Some(code) = refuse_agent(CMD, &Agent::parse(agent)) {
        return code;
    }
    let args = remove_args();
    let Some(claude) = find_claude() else {
        eprintln!("{CMD}: {}", t("mcp.claude-missing", &[]));
        println!("{}", shown_command(&args));
        return ExitCode::FAILURE;
    };
    let server = server_path().unwrap_or_default();
    match registration(&claude) {
        Registration::Missing => {
            println!("{}", t("mcp.uninstall.absent", &[]));
            return ExitCode::SUCCESS;
        }
        found if found.is_ours(&server) => {}
        Registration::Unknown => {
            eprintln!("{CMD}: {}", t("mcp.claude-changed", &[]));
            println!("{}", shown_command(&args));
            return ExitCode::FAILURE;
        }
        Registration::User { .. } | Registration::OtherScope => {
            eprintln!("{CMD}: {}", t("mcp.foreign-uninstall", &[]));
            return ExitCode::FAILURE;
        }
    }
    println!("{}", t("mcp.uninstall.plan", &[]));
    println!("  {}", shown_command(&args));
    match run_claude(&claude, &args) {
        Some(out) if out.success => {
            println!("{}", t("mcp.uninstall.done", &[]));
            ExitCode::SUCCESS
        }
        Some(out) => claude_failed(CMD, &out),
        None => {
            eprintln!("{CMD}: {}", t("mcp.claude-missing", &[]));
            println!("{}", shown_command(&args));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real answers of Claude Code 2.1.284 (S-MCP-5), captured with a
    // temporary CLAUDE_CONFIG_DIR.
    const OURS: &str = include_str!("../tests/fixtures/claude-2.1.284/get-ours.stdout");
    const FOREIGN: &str = include_str!("../tests/fixtures/claude-2.1.284/get-foreign.stdout");
    const PROJECT: &str = include_str!("../tests/fixtures/claude-2.1.284/get-project.stdout");
    const MISSING: &str = include_str!("../tests/fixtures/claude-2.1.284/get-missing.stderr");

    #[test]
    fn parses_a_registered_server() {
        let found = Registration::parse(true, &OURS.replace("{SERVER}", "/opt/bin/raptor-mcp"), "");
        assert_eq!(
            found,
            Registration::User {
                stdio: true,
                command: "/opt/bin/raptor-mcp".into(),
                args: String::new(),
                env: vec![],
            }
        );
    }

    #[test]
    fn parses_args_and_environment() {
        let found = Registration::parse(true, FOREIGN, "");
        assert_eq!(
            found,
            Registration::User {
                stdio: true,
                command: "/usr/bin/true".into(),
                args: "--flag two".into(),
                env: vec!["TOKEN=x".into(), "B=y".into()],
            }
        );
        assert!(
            !found.is_ours(Path::new("/usr/bin/true")),
            "args and env make it foreign"
        );
    }

    #[test]
    fn a_missing_server_and_another_scope() {
        assert_eq!(
            Registration::parse(false, "", MISSING),
            Registration::Missing
        );
        assert_eq!(
            Registration::parse(true, PROJECT, ""),
            Registration::OtherScope
        );
    }

    #[test]
    fn an_unknown_shape_is_not_guessed() {
        assert_eq!(
            Registration::parse(true, "{\"gitraptor\":{}}", ""),
            Registration::Unknown
        );
        assert_eq!(
            Registration::parse(false, "", "boom"),
            Registration::Unknown
        );
        let no_command = OURS.replace("  Command: {SERVER}\n", "");
        assert_eq!(
            Registration::parse(true, &no_command, ""),
            Registration::Unknown
        );
    }

    #[test]
    fn ownership_compares_canonical_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("raptor-mcp");
        std::fs::write(&real, "").unwrap();
        let alias = tmp.path().join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        #[cfg(not(unix))]
        std::fs::copy(&real, &alias).unwrap();
        let found = Registration::parse(
            true,
            &OURS.replace("{SERVER}", &alias.display().to_string()),
            "",
        );
        #[cfg(unix)]
        assert!(found.is_ours(&real));
        assert!(!found.is_ours(&tmp.path().join("other")));
    }

    #[test]
    fn agents() {
        assert_eq!(Agent::parse("claude-code"), Agent::ClaudeCode);
        assert_eq!(Agent::parse("Cursor"), Agent::Unsupported("Cursor".into()));
        assert_eq!(Agent::parse("codex"), Agent::Unsupported("Codex".into()));
        assert_eq!(
            Agent::parse("copilot"),
            Agent::Unsupported("Copilot".into())
        );
        assert_eq!(
            Agent::parse("evil\u{202e}"),
            Agent::Unsupported(sanitize("evil\u{202e}"))
        );
    }

    #[test]
    fn volatile_locations() {
        assert!(is_volatile(Path::new(
            "/Users/dev/.npm/_npx/1a2b/node_modules/.bin/raptor-mcp"
        )));
        assert!(is_volatile(Path::new(
            "/Users/dev/Library/Caches/pnpm/dlx/abc/raptor-mcp"
        )));
        assert!(is_volatile(&std::env::temp_dir().join("x/raptor-mcp")));
        assert!(!is_volatile(Path::new("/opt/homebrew/bin/raptor-mcp")));
        assert!(!is_volatile(Path::new("/Users/dev/.local/bin/raptor-mcp")));
    }

    #[test]
    fn the_shown_command_is_quoted() {
        let args = add_args(Path::new("/Users/a b/it's/raptor-mcp"));
        assert_eq!(
            shown_command(&args),
            "claude mcp add --scope user --transport stdio gitraptor -- '/Users/a b/it'\\''s/raptor-mcp'"
        );
        assert_eq!(
            shown_command(&remove_args()),
            "claude mcp remove --scope user gitraptor"
        );
    }
}
