//! US-MCP-001: `raptor mcp install` and `raptor mcp uninstall`, one test per
//! scenario. Claude Code is a stateful fake on `PATH` that answers like
//! Claude Code 2.1.284 (the shape is pinned by `tests/fixtures`) and logs
//! its argv and cwd. Temporary folders and profile only (NFR-01).
//!
//! Unix only (the fake is a shell script). Windows: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const OURS: &str = include_str!("fixtures/claude-2.1.284/get-ours.stdout");
const FOREIGN: &str = include_str!("fixtures/claude-2.1.284/get-foreign.stdout");

const FAKE_CLAUDE: &str = r#"#!/bin/sh
s="$FAKE_CLAUDE_STATE"
pwd -P >> "$s/cwd.log"
echo "$*" >> "$s/argv.log"
case "$1 $2" in
"mcp get")
  if [ -f "$s/servers/$3" ]; then cat "$s/servers/$3"; exit 0; fi
  echo "No MCP server named \"$3\". Run \`claude mcp add\` to add one." >&2; exit 1 ;;
"mcp add")
  name="$7"; cmd="$9"
  if [ -f "$s/servers/$name" ]; then echo "MCP server $name already exists in user config" >&2; exit 1; fi
  sed "s|{SERVER}|$cmd|" "$s/template" > "$s/servers/$name"
  echo "Added stdio MCP server $name with command: $cmd  to user config"; exit 0 ;;
"mcp remove")
  name="$5"
  if [ ! -f "$s/servers/$name" ]; then echo "No MCP server named \"$name\" in user scope" >&2; exit 1; fi
  rm "$s/servers/$name"; echo "Removed MCP server $name from user config"; exit 0 ;;
esac
exit 2
"#;

/// `raptor-mcp` next to `raptor`; built there if this package was tested alone.
fn server() -> PathBuf {
    gitraptor_testkit::sibling_bin(Path::new(RAPTOR), "gitraptor-mcp", "raptor-mcp")
}

struct Env {
    tmp: tempfile::TempDir,
    with_claude: bool,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("claude");
        fs::create_dir_all(state.join("servers")).unwrap();
        fs::write(state.join("template"), OURS).unwrap();
        let bin = tmp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("claude");
        fs::write(&fake, FAKE_CLAUDE).unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir_all(tmp.path().join("repo")).unwrap();
        Self {
            tmp,
            with_claude: true,
        }
    }

    fn state(&self) -> PathBuf {
        self.tmp.path().join("claude")
    }

    fn profile(&self) -> PathBuf {
        self.tmp.path().join("profile")
    }

    fn servers(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = fs::read_dir(self.state().join("servers"))
            .unwrap()
            .map(|e| e.unwrap())
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    fs::read_to_string(e.path()).unwrap(),
                )
            })
            .collect();
        out.sort();
        out
    }

    fn seed(&self, name: &str, get_output: &str) {
        fs::write(self.state().join("servers").join(name), get_output).unwrap();
    }

    fn argv(&self) -> Vec<String> {
        fs::read_to_string(self.state().join("argv.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn run_with(&self, exe: &Path, args: &[&str]) -> Output {
        let path = if self.with_claude {
            format!("{}:/usr/bin:/bin", self.tmp.path().join("bin").display())
        } else {
            "/usr/bin:/bin".to_owned()
        };
        Command::new(exe)
            .args(args)
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.tmp.path())
            .env("FAKE_CLAUDE_STATE", self.state())
            .env("GITRAPTOR_PROFILE_DIR", self.profile())
            .env("LANG", "en_US.UTF-8")
            .current_dir(self.tmp.path().join("repo"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn raptor(&self, args: &[&str]) -> Output {
        server();
        self.run_with(Path::new(RAPTOR), args)
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(tree(&path));
        } else {
            out.push((path.clone(), fs::read(&path).unwrap()));
        }
    }
    out.sort();
    out
}

/// Escenario: El desarrollador instala el servidor en Claude Code.
#[test]
fn install_registers_gitraptor_in_the_user_scope() {
    let env = Env::new();
    let repo = env.tmp.path().join("repo");
    fs::write(repo.join("README.md"), "hello").unwrap();
    let repo_before = tree(&repo);
    env.seed("other-a", &FOREIGN.replace("/usr/bin/true", "/opt/a"));

    let out = env.raptor(&["mcp", "install"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    let server = server();
    // It says what it will change before changing it.
    let plan = text.find("Running:").expect(&text);
    let done = text.find("gitraptor registered").expect(&text);
    assert!(plan < done, "{text}");
    assert!(text.contains(&format!(
        "claude mcp add --scope user --transport stdio gitraptor -- {}",
        server.display()
    )));

    // Fixed argv, absolute path of the installed binary, run from `/`.
    let argv = env.argv();
    assert_eq!(
        argv,
        vec![
            "mcp get gitraptor".to_owned(),
            format!(
                "mcp add --scope user --transport stdio gitraptor -- {}",
                server.display()
            ),
        ]
    );
    let cwd = fs::read_to_string(env.state().join("cwd.log")).unwrap();
    assert!(cwd.lines().all(|l| l == "/"), "{cwd}");

    // The other servers are exactly as they were.
    let servers = env.servers();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[1].0, "other-a");
    assert_eq!(servers[1].1, FOREIGN.replace("/usr/bin/true", "/opt/a"));
    assert!(
        servers[0]
            .1
            .contains(&format!("Command: {}", server.display()))
    );

    // No repo file changed, no profile, so nothing in the MCP allowlist.
    assert_eq!(tree(&repo), repo_before);
    assert!(!env.profile().exists());
}

/// Escenario: Instalar dos veces no cambia nada.
#[test]
fn installing_twice_changes_nothing() {
    let env = Env::new();
    assert!(env.raptor(&["mcp", "install"]).status.success());
    let before = env.servers();

    let out = env.raptor(&["mcp", "install"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("gitraptor is already installed in Claude Code; nothing to do"),
        "{}",
        stdout(&out)
    );
    assert_eq!(env.servers(), before);
    assert_eq!(
        env.argv()
            .iter()
            .filter(|a| a.starts_with("mcp add"))
            .count(),
        1
    );
}

/// Escenario: El desarrollador retira el servidor.
#[test]
fn uninstall_removes_only_gitraptor() {
    let env = Env::new();
    env.seed("other-a", &FOREIGN.replace("/usr/bin/true", "/opt/a"));
    env.seed("other-b", &FOREIGN.replace("/usr/bin/true", "/opt/b"));
    assert!(env.raptor(&["mcp", "install"]).status.success());
    let others: Vec<_> = env
        .servers()
        .into_iter()
        .filter(|(n, _)| n != "gitraptor")
        .collect();

    let out = env.raptor(&["mcp", "uninstall"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("gitraptor removed from Claude Code"));
    assert_eq!(env.servers(), others);
    assert_eq!(
        env.argv().last().unwrap(),
        "mcp remove --scope user gitraptor"
    );

    // Again: nothing to do.
    let out = env.raptor(&["mcp", "uninstall"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("not installed in Claude Code; nothing to do"));
}

fn assert_refused_unchanged(env: &Env, out: &Output, reason: &str, action: &str) {
    assert!(!out.status.success());
    let err = stderr(out);
    assert!(err.contains(reason), "{err}");
    assert!(err.contains(action), "{err}");
    assert!(
        !env.argv()
            .iter()
            .any(|a| a.starts_with("mcp add") || a.starts_with("mcp remove")),
        "the Claude Code configuration was not touched"
    );
}

/// Esquema: la instalación no cambia nada cuando no puede hacerlo de forma
/// segura — Cursor, Codex y Copilot.
#[test]
fn unsupported_agents_are_refused() {
    for (agent, name) in [
        ("cursor", "Cursor"),
        ("codex", "Codex"),
        ("copilot", "Copilot"),
    ] {
        let env = Env::new();
        let out = env.raptor(&["mcp", "install", "--agent", agent]);
        assert_refused_unchanged(
            &env,
            &out,
            &format!("{name} is not supported yet"),
            "use --agent claude-code",
        );
        assert!(env.argv().is_empty(), "Claude Code is not even asked");
    }
}

/// Esquema: GitRaptor se ejecuta desde una caché de npx o una carpeta
/// temporal → "instala GitRaptor primero".
#[test]
fn a_binary_in_a_temporary_folder_is_refused() {
    let env = Env::new();
    let copy = env.tmp.path().join("unpacked");
    fs::create_dir_all(&copy).unwrap();
    for bin in [PathBuf::from(RAPTOR), server()] {
        let to = copy.join(bin.file_name().unwrap());
        fs::copy(&bin, &to).unwrap();
        fs::set_permissions(&to, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = env.run_with(&copy.join("raptor"), &["mcp", "install"]);
    assert_refused_unchanged(
        &env,
        &out,
        "temporary folder or an npx cache",
        "install GitRaptor first",
    );
    assert!(env.servers().is_empty());
}

/// Esquema: Claude Code ya tiene un "gitraptor" que apunta a otro binario.
#[test]
fn a_foreign_gitraptor_is_never_overwritten() {
    let env = Env::new();
    let foreign = OURS.replace("{SERVER}", "/somewhere/else/raptor-mcp");
    env.seed("gitraptor", &foreign);

    let out = env.raptor(&["mcp", "install"]);
    assert_refused_unchanged(
        &env,
        &out,
        "a gitraptor server that is not this GitRaptor already exists",
        "remove it with claude mcp remove gitraptor and install again",
    );
    assert_eq!(
        env.servers(),
        vec![("gitraptor".to_owned(), foreign.clone())]
    );

    // Uninstall does not remove it either.
    let out = env.raptor(&["mcp", "uninstall"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("is not this GitRaptor's"));
    assert_eq!(env.servers(), vec![("gitraptor".to_owned(), foreign)]);
}

/// Our binary, but with extra args or environment, is not ours either.
#[test]
fn our_path_with_extra_args_is_foreign() {
    let env = Env::new();
    let server = server();
    let tampered = FOREIGN.replace("/usr/bin/true", &server.display().to_string());
    env.seed("gitraptor", &tampered);
    let out = env.raptor(&["mcp", "install"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not this GitRaptor"));
    assert_eq!(env.servers(), vec![("gitraptor".to_owned(), tampered)]);
}

/// S-MCP-5: an answer of an unknown shape changes nothing.
#[test]
fn an_unknown_claude_answer_changes_nothing() {
    let env = Env::new();
    env.seed("gitraptor", "{\"gitraptor\": {\"command\": \"x\"}}\n");
    let out = env.raptor(&["mcp", "install"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("form this GitRaptor does not know"));
    assert!(stdout(&out).contains("claude mcp add --scope user"));
    assert!(!env.argv().iter().any(|a| a.starts_with("mcp add")));
}

/// Escenario: Sin la CLI de Claude Code, el desarrollador recibe el comando
/// exacto.
#[test]
fn without_claude_the_exact_command_is_shown() {
    let mut env = Env::new();
    env.with_claude = false;
    let out = env.raptor(&["mcp", "install"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("Claude Code CLI not found; nothing was changed"));
    assert_eq!(
        stdout(&out).trim(),
        format!(
            "claude mcp add --scope user --transport stdio gitraptor -- {}",
            server().display()
        )
    );
    assert!(env.argv().is_empty());
    assert!(!env.tmp.path().join(".claude.json").exists());
    assert!(!env.tmp.path().join(".claude").exists());
}

/// A `claude` that others can modify is not run.
#[test]
fn a_group_writable_claude_is_not_trusted() {
    let env = Env::new();
    let fake = env.tmp.path().join("bin").join("claude");
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o775)).unwrap();
    let out = env.raptor(&["mcp", "install"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("Claude Code CLI not found"));
    assert!(env.argv().is_empty());
}

/// Messages follow the locale (NFR-10).
#[test]
fn messages_in_spanish() {
    let env = Env::new();
    let out = Command::new(RAPTOR)
        .args(["mcp", "install", "--agent", "cursor"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "es_ES.UTF-8")
        .current_dir(env.tmp.path())
        .output()
        .unwrap();
    assert!(
        stderr(&out).contains("Cursor no está soportado todavía"),
        "{}",
        stderr(&out)
    );
}
