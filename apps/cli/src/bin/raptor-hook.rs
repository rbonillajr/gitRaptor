//! `raptor-hook`: the native Guardrails dispatcher (ADR-GRD-001 § 2 and § 3, Enmienda
//! 2026-10-05). A copy of this program is each dispatcher in `<common>/gitraptor/hooks/`; its
//! constants are in `../dispatch.conf`, found from its own executable path, never from the
//! environment.
//!
//! - `reference-transaction` outside `prepared`: exits 0 without reading anything else.
//! - Fast path: refs that are not governed and prunes of `pack-refs` exit 0 here, without
//!   starting `raptor` (the classification is the same file `crates/policy` compiles).
//! - Otherwise `raptor hook` decides: exit 0 allows, 1 denies, anything else is an internal
//!   error.
//! - Without `raptor` (or on an internal error): fail-closed for `pre-push`, `pre-rebase` and the
//!   deletion of a branch; anything else passes with a warning (decision 4 of Rene Bonilla).
//!
//! Plain `std` on purpose: it runs on every Git operation of a protected repo. This is the one
//! named exception to "no own process launch" of the hook layer (ADR-GRD-001 § 7, Enmienda
//! 2026-10-05): it only ever starts the `raptor` of its constants.

#[path = "../../../../crates/policy/src/guard/fastpath.rs"]
#[allow(dead_code)]
mod fastpath;

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// Longest accepted input line (as `crates/policy::guard::input`).
const MAX_LINE: usize = 8 * 1024;
const MAX_CONF: u64 = 64 * 1024;
const MAX_INPUT: u64 = 256 * 1024 * 1024;
/// Template versions this dispatcher understands (ADR-GRD-001 § 8).
const TEMPLATES: &[&str] = &["1", "2"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hook {
    PrePush,
    PreRebase,
    ReferenceTransaction,
    /// Commit authorship (US-GRD-018, template 2).
    PreCommit,
    CommitMsg,
}

impl Hook {
    fn from_exe(exe: &Path) -> Option<Self> {
        match exe.file_stem()?.to_str()? {
            "pre-push" => Some(Self::PrePush),
            "pre-rebase" => Some(Self::PreRebase),
            "reference-transaction" => Some(Self::ReferenceTransaction),
            "pre-commit" => Some(Self::PreCommit),
            "commit-msg" => Some(Self::CommitMsg),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::PrePush => "pre-push",
            Self::PreRebase => "pre-rebase",
            Self::ReferenceTransaction => "reference-transaction",
            Self::PreCommit => "pre-commit",
            Self::CommitMsg => "commit-msg",
        }
    }
}

/// The constants, in the order `raptor hook` takes them.
const KEYS: [&str; 8] = [
    "template", "hook", "repo", "common", "channel", "instance", "state", "prior",
];

struct Conf {
    values: Vec<(String, String)>,
}

impl Conf {
    fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// `dispatch.conf`: a regular file (never a link), bounded, strict `key<TAB>value` lines.
    fn read(path: &Path) -> Option<Self> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        if !meta.is_file() || meta.len() > MAX_CONF {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let mut values: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            let (k, v) = line.split_once('\t')?;
            if v.contains('\t') || values.iter().any(|(seen, _)| seen == k) {
                return None;
            }
            values.push((k.to_owned(), v.to_owned()));
        }
        let conf = Self { values };
        for key in [
            "template", "raptor", "repo", "common", "channel", "instance", "state", "prior",
        ] {
            conf.get(key)?;
        }
        TEMPLATES.contains(&conf.get("template")?).then_some(conf)
    }
}

fn spanish() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .is_some_and(|lang| lang.starts_with("es"))
}

/// Only printable characters of our own constant reach the terminal.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}

enum Msg<'a> {
    Missing(&'a str),
    Inactive(&'a str),
    Internal,
    InternalPassed,
    Moved,
}

fn say(msg: Msg<'_>) {
    let es = spanish();
    let text = match msg {
        Msg::Missing(path) if es => format!(
            "GitRaptor: este repo está protegido y GitRaptor no se encuentra en «{}». La operación no se ejecuta. Restaura o reinstala GitRaptor y reintenta.",
            clean(path)
        ),
        Msg::Missing(path) => format!(
            "GitRaptor: this repo is protected and GitRaptor was not found at «{}». The operation did not run. Restore or reinstall GitRaptor and retry.",
            clean(path)
        ),
        Msg::Inactive(path) if es => format!(
            "GitRaptor: protección inactiva: no se encuentra GitRaptor en «{}».",
            clean(path)
        ),
        Msg::Inactive(path) => format!(
            "GitRaptor: protection inactive: GitRaptor was not found at «{}».",
            clean(path)
        ),
        Msg::Internal if es => {
            "GitRaptor: error interno al decidir. La operación no se ejecuta.".to_owned()
        }
        Msg::Internal => "GitRaptor: internal error while deciding. The operation did not run.".to_owned(),
        Msg::InternalPassed if es => {
            "GitRaptor: error interno; protección inactiva para esta operación.".to_owned()
        }
        Msg::InternalPassed => {
            "GitRaptor: internal error; protection inactive for this operation.".to_owned()
        }
        Msg::Moved if es => {
            "GitRaptor: los hooks de este repositorio se movieron o alteraron. La operación no se ejecuta.".to_owned()
        }
        Msg::Moved => {
            "GitRaptor: the hooks of this repository were moved or altered. The operation did not run.".to_owned()
        }
    };
    let _ = writeln!(std::io::stderr(), "{text}");
}

/// ADR-GRD-001 § 3: without a decision from `raptor`, fail-closed only where the operation is
/// risky; `missing` says whether `raptor` is absent (or an internal error happened).
fn fallback(hook: Hook, input: &[u8], common: &Path, raptor: &str, missing: bool) -> ExitCode {
    let deny = match hook {
        Hook::PrePush | Hook::PreRebase => true,
        Hook::ReferenceTransaction => fastpath::deletes_a_branch(input, common, MAX_LINE),
        // A commit is not risky by itself: it passes with the warning (decision 4).
        Hook::PreCommit | Hook::CommitMsg => false,
    };
    match (deny, missing) {
        (true, true) => say(Msg::Missing(raptor)),
        (true, false) => say(Msg::Internal),
        (false, true) => say(Msg::Inactive(raptor)),
        (false, false) => say(Msg::InternalPassed),
    }
    if deny {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        #[cfg(unix)]
        Ok(m) => {
            use std::os::unix::fs::PermissionsExt;
            m.is_file() && m.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        Ok(m) => m.is_file(),
        Err(_) => false,
    }
}

/// The environment of `raptor hook`, built from scratch (ADR-GRD-001 § 2): `GIT_DIR` and
/// `GIT_INDEX_FILE` if Git set them, the language of the messages, and on Windows what a
/// process needs to start.
fn allowlisted_env() -> Vec<(OsString, OsString)> {
    let mut keys = vec!["GIT_DIR", "GIT_INDEX_FILE", "LC_ALL", "LC_MESSAGES", "LANG"];
    if cfg!(windows) {
        keys.push("SystemRoot");
    }
    keys.into_iter()
        .filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v)))
        .collect()
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(exe) = std::env::current_exe().ok() else {
        return ExitCode::FAILURE;
    };
    let Some(hook) = Hook::from_exe(&exe) else {
        // A copy under a name this dispatcher does not serve: nothing to decide.
        return ExitCode::SUCCESS;
    };
    let prepared = args.first().map(OsString::as_os_str) == Some(OsStr::new("prepared"));
    // L-01: outside `prepared`, nothing to evaluate and (US-GRD-001) no prior hook to chain.
    if hook == Hook::ReferenceTransaction && !prepared {
        return ExitCode::SUCCESS;
    }
    let mut input = Vec::new();
    // `pre-rebase`, `pre-commit` and `commit-msg` take no input.
    if matches!(hook, Hook::PrePush | Hook::ReferenceTransaction) {
        // One byte past the bound means the input was cut: never decide on a prefix (Git
        // ignores a hook that stops reading).
        let read = std::io::stdin()
            .lock()
            .take(MAX_INPUT + 1)
            .read_to_end(&mut input);
        if read.is_err() || input.len() as u64 > MAX_INPUT {
            say(Msg::Internal);
            return ExitCode::FAILURE;
        }
    }
    // <common>/gitraptor/hooks/<hook>
    let folder = exe.parent().and_then(Path::parent);
    let common_here = folder
        .and_then(Path::parent)
        .and_then(|c| c.canonicalize().ok())
        .map(fastpath::simplified);
    let conf = folder.and_then(|f| Conf::read(&f.join("dispatch.conf")));
    let Some(conf) = conf else {
        return fallback(
            hook,
            &input,
            common_here.as_deref().unwrap_or(Path::new("")),
            "",
            true,
        );
    };
    let common = PathBuf::from(conf.get("common").unwrap_or_default());
    let raptor = conf.get("raptor").unwrap_or_default();
    // M-02: the constants must be the ones of the repo this dispatcher sits in.
    if common_here.as_deref() != Some(common.as_path()) {
        say(Msg::Moved);
        return ExitCode::FAILURE;
    }
    let skippable = match hook {
        Hook::ReferenceTransaction => {
            fastpath::skippable_ref_transaction(&input, &common, MAX_LINE)
        }
        Hook::PrePush => fastpath::skippable_push(&input, MAX_LINE),
        Hook::PreRebase | Hook::PreCommit | Hook::CommitMsg => false,
    };
    if skippable {
        return ExitCode::SUCCESS;
    }
    if !is_executable(Path::new(raptor)) {
        return fallback(hook, &input, &common, raptor, true);
    }
    let mut command = Command::new(raptor);
    command.arg("hook");
    for key in KEYS {
        if key == "hook" {
            command.arg(hook.name());
        } else {
            command.arg(conf.get(key).unwrap_or_default());
        }
    }
    command
        .arg("--")
        .args(&args)
        .env_clear()
        .envs(allowlisted_env())
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let Ok(mut child) = command.spawn() else {
        return fallback(hook, &input, &common, raptor, false);
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A `raptor` that exits early closes the pipe: its exit code still decides.
        let _ = stdin.write_all(&input);
    }
    match child.wait().ok().and_then(|s| s.code()) {
        Some(0) => ExitCode::SUCCESS,
        Some(1) => ExitCode::FAILURE,
        // A signal, a panic or an unexpected code: an internal error (ADR-GRD-001 § 3).
        _ => fallback(hook, &input, &common, raptor, false),
    }
}
