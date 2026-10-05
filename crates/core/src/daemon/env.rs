//! The environment the daemon trusts (SEC-10).
//!
//! The daemon may be launched by a client that inherited a hostile
//! environment (`GIT_*`, `LD_PRELOAD`, `DYLD_*`, `XDG_CONFIG_HOME`, a
//! relative `PATH`). Those can only affect the start of the process itself:
//! the environment is captured once, by allowlist, and everything afterwards
//! (Git resolution, child processes) reads this capture, never the process
//! environment. This is the only file of the daemon allowed to read the
//! process environment (checked by `env_reads_only_in_capture`).

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use gitraptor_git::Invoker;
use gitraptor_git::resolve::ResolveConfig;

/// Variables kept from the inherited environment. `PATH` is kept separately
/// and only with its absolute entries.
#[cfg(not(windows))]
const ALLOWED: &[&str] = &["HOME", "USER", "LOGNAME", "TMPDIR"];
#[cfg(windows)]
const ALLOWED: &[&str] = &[
    "SystemRoot",
    "SystemDrive",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramFiles",
    "TEMP",
    "TMP",
];

/// Allowlisted snapshot of the environment.
#[derive(Clone, PartialEq, Eq)]
pub struct DaemonEnv {
    vars: Vec<(OsString, OsString)>,
}

impl std::fmt::Debug for DaemonEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Even allowlisted values stay out of logs and debug output (SEC-05).
        let names: Vec<_> = self.vars.iter().map(|(k, _)| k).collect();
        f.debug_struct("DaemonEnv").field("names", &names).finish()
    }
}

impl DaemonEnv {
    /// Captures the process environment by allowlist.
    pub fn capture() -> Self {
        Self::from_vars(std::env::vars_os())
    }

    /// Builds the allowlisted environment from `vars` (tests pass their own).
    pub fn from_vars(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        let mut kept = Vec::new();
        for (key, value) in vars {
            if key_is(&key, "PATH") {
                let absolute: Vec<PathBuf> = std::env::split_paths(&value)
                    .filter(|p| p.is_absolute())
                    .collect();
                // Nothing absolute left: no `PATH` at all, so resolution falls back to the
                // well-known locations instead of reading an empty entry.
                if absolute.is_empty() {
                    continue;
                }
                if let Ok(joined) = std::env::join_paths(absolute) {
                    kept.push((OsString::from("PATH"), joined));
                }
            } else if let Some(name) = ALLOWED.iter().find(|name| key_is(&key, name)) {
                kept.push((OsString::from(name), value));
            }
        }
        Self { vars: kept }
    }

    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.vars
            .iter()
            .find(|(k, _)| key_is(k, name))
            .map(|(_, v)| v.as_os_str())
    }

    pub fn vars(&self) -> &[(OsString, OsString)] {
        &self.vars
    }

    /// Launcher for Git children: their environment is built from this
    /// capture, never from the process environment.
    pub fn invoker(&self) -> Invoker {
        Invoker::default().with_parent_env(self.vars.clone())
    }

    /// Git resolution for the running OS, with `PATH` taken from this capture
    /// (absolute entries only). An absent `PATH` falls back to the well-known
    /// locations (launchd and systemd start with a minimal `PATH`).
    pub fn git_resolve_config(&self, configured_path: Option<PathBuf>) -> ResolveConfig {
        let mut config = ResolveConfig::for_current_os(configured_path);
        config.path_env = self.get("PATH").map(OsStr::to_os_string);
        config
    }
}

/// Debug-build test hook: `GITRAPTOR_AGENT_EXECUTABLES` replaces the agent
/// classifier with a `:`-separated list of executable names, so tests use a
/// simulated agent and are not refused because a real Claude Code session
/// runs them. Release builds do not even read it, like
/// `GITRAPTOR_PROFILE_DIR` (SEC-06).
pub const AGENT_EXECUTABLES_ENV: &str = "GITRAPTOR_AGENT_EXECUTABLES";

pub(crate) fn agent_executables_override() -> Option<Vec<String>> {
    if cfg!(debug_assertions) {
        parse_agent_override(std::env::var_os(AGENT_EXECUTABLES_ENV).as_deref())
    } else {
        None
    }
}

fn parse_agent_override(value: Option<&OsStr>) -> Option<Vec<String>> {
    let names: Vec<String> = value?
        .to_str()?
        .split(':')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .collect();
    (!names.is_empty()).then_some(names)
}

/// Environment names are case-insensitive on Windows.
fn key_is(key: &OsStr, name: &str) -> bool {
    if cfg!(windows) {
        key.to_str().is_some_and(|k| k.eq_ignore_ascii_case(name))
    } else {
        key == name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The current dir, a relative entry and one absolute directory, in the OS's own syntax.
    #[cfg(unix)]
    const HOSTILE_PATH: &str = ".:relative/bin:/usr/bin";
    #[cfg(windows)]
    const HOSTILE_PATH: &str = r".;relative\bin;\rooted\bin;C:\Windows\System32";

    fn hostile() -> Vec<(OsString, OsString)> {
        [
            ("GIT_EXEC_PATH", "/tmp/evil"),
            ("GIT_CONFIG_PARAMETERS", "'core.fsmonitor'='/tmp/evil'"),
            ("LD_PRELOAD", "/tmp/evil.so"),
            ("DYLD_INSERT_LIBRARIES", "/tmp/evil.dylib"),
            ("XDG_CONFIG_HOME", "/tmp/evil-config"),
            ("AWS_SECRET_ACCESS_KEY", "s3cr3t"),
            ("PATH", HOSTILE_PATH),
            ("HOME", "/home/u"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
    }

    #[test]
    fn hostile_variables_are_dropped() {
        let env = DaemonEnv::from_vars(hostile());
        for name in [
            "GIT_EXEC_PATH",
            "GIT_CONFIG_PARAMETERS",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "XDG_CONFIG_HOME",
            "AWS_SECRET_ACCESS_KEY",
        ] {
            assert_eq!(env.get(name), None, "{name} must not be kept");
        }
        #[cfg(unix)]
        assert_eq!(env.get("PATH"), Some(OsStr::new("/usr/bin")));
        #[cfg(windows)]
        assert_eq!(env.get("PATH"), Some(OsStr::new(r"C:\Windows\System32")));
    }

    #[test]
    fn a_path_without_absolute_entries_is_dropped() {
        let env = DaemonEnv::from_vars([("PATH".into(), "relative/bin".into())]);
        assert_eq!(env.get("PATH"), None);
        assert_eq!(env.git_resolve_config(None).path_env, None);
    }

    #[test]
    fn git_children_and_resolution_only_see_the_capture() {
        let env = DaemonEnv::from_vars(hostile());
        let child = env.invoker().child_env();
        for (key, _) in &child {
            let key = key.to_string_lossy();
            assert!(
                ![
                    "GIT_EXEC_PATH",
                    "LD_PRELOAD",
                    "DYLD_INSERT_LIBRARIES",
                    "XDG_CONFIG_HOME"
                ]
                .contains(&key.as_ref()),
                "{key} reached a child"
            );
        }
        let config = env.git_resolve_config(None);
        let path = config.path_env.unwrap();
        assert!(std::env::split_paths(&path).all(|p| p.is_absolute()));
    }

    #[test]
    fn debug_output_never_shows_values() {
        let env = DaemonEnv::from_vars(hostile());
        let shown = format!("{env:?}");
        assert!(!shown.contains("/home/u"));
        assert!(!shown.contains("s3cr3t"));
    }

    #[test]
    fn agent_override_parses_names() {
        assert_eq!(
            parse_agent_override(Some(OsStr::new("fake-agent: other "))),
            Some(vec!["fake-agent".to_owned(), "other".to_owned()])
        );
        assert_eq!(parse_agent_override(Some(OsStr::new(" : "))), None);
        assert_eq!(parse_agent_override(None), None);
    }

    /// Only `DaemonEnv::capture` reads the process environment (SEC-10).
    #[test]
    fn env_reads_only_in_capture() {
        let files = [
            ("mod.rs", include_str!("mod.rs")),
            ("lock.rs", include_str!("lock.rs")),
            ("log.rs", include_str!("log.rs")),
            ("shutdown.rs", include_str!("shutdown.rs")),
            ("state.rs", include_str!("state.rs")),
        ];
        let banned = [["env", "::var"].concat(), ["env", "::vars"].concat()];
        for (name, source) in files {
            for (n, line) in source.lines().enumerate() {
                for b in &banned {
                    assert!(
                        !line.contains(b.as_str()),
                        "{name}:{}: read the environment through DaemonEnv",
                        n + 1
                    );
                }
            }
        }
    }
}
