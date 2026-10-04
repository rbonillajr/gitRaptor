//! Validation of what clients send (SEC-02, ADR-GRP-005 § 5 and § 6.6).
//!
//! Paths are checked lexically before anything touches the file system:
//! canonicalizing a UNC path on Windows opens an SMB connection (M9). Only
//! then are they canonicalized and checked against the observed worktrees
//! (BR-VAL-002). Refs follow `check-ref-format` (`crates/git::refname`).

use std::path::{Path, PathBuf};

use gitraptor_git::refname::RefName;

/// Longest accepted path from a client, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;

/// Why a client value was refused. The message never echoes the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    Empty,
    TooLong,
    NotAbsolute,
    ControlCharacter,
    /// `\\server\share`, `//server/share`, `\\?\` or `\\.\` prefixes.
    UncOrDevice,
    /// A Windows device name (`CON`, `NUL`, `COM1`, …) as a component.
    DeviceName,
    /// An alternate data stream (`file:stream`).
    AlternateStream,
    /// Exists nowhere, or resolves outside every observed worktree.
    OutsideObserved,
    InvalidRef,
    ReservedName,
}

impl Invalid {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLong => "too long",
            Self::NotAbsolute => "not an absolute path",
            Self::ControlCharacter => "control character",
            Self::UncOrDevice => "UNC or device path",
            Self::DeviceName => "device name",
            Self::AlternateStream => "alternate data stream",
            Self::OutsideObserved => "outside the observed repos",
            Self::InvalidRef => "invalid ref name",
            Self::ReservedName => "reserved name",
        }
    }
}

const DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9", "conin$",
    "conout$",
];

/// Lexical checks of a client path. Never touches the file system. UNC and
/// device prefixes are refused on every OS; device names and alternate data
/// streams are Windows rules, applied on Windows and to any Windows-shaped
/// path (a folder called `aux` is fine on macOS).
pub fn client_path(raw: &str) -> Result<PathBuf, Invalid> {
    if raw.is_empty() {
        return Err(Invalid::Empty);
    }
    if raw.len() > MAX_PATH_BYTES {
        return Err(Invalid::TooLong);
    }
    if raw.chars().any(char::is_control) {
        return Err(Invalid::ControlCharacter);
    }
    if raw.starts_with("\\\\") || raw.starts_with("//") || raw.starts_with("\\??\\") {
        return Err(Invalid::UncOrDevice);
    }
    let windows_drive = raw.len() >= 3
        && raw.as_bytes()[0].is_ascii_alphabetic()
        && raw.as_bytes()[1] == b':'
        && matches!(raw.as_bytes()[2], b'\\' | b'/');
    let windows_rules = cfg!(windows) || windows_drive || raw.contains('\\');
    if windows_rules {
        for part in raw.split(['/', '\\']).filter(|p| !p.is_empty()) {
            let base = part.split('.').next().unwrap_or(part).trim_end();
            if DEVICE_NAMES.contains(&base.to_ascii_lowercase().as_str()) {
                return Err(Invalid::DeviceName);
            }
        }
        let rest = if windows_drive { &raw[2..] } else { raw };
        if rest.contains(':') {
            return Err(Invalid::AlternateStream);
        }
    }
    let path = PathBuf::from(raw);
    if !(path.is_absolute() || windows_drive) {
        return Err(Invalid::NotAbsolute);
    }
    Ok(path)
}

/// Canonicalizes a lexically valid path and returns it only if it lies
/// inside one of `observed` (canonical worktree roots).
pub fn observed_path(raw: &str, observed: &[PathBuf]) -> Result<PathBuf, Invalid> {
    let path = client_path(raw)?;
    let canonical = std::fs::canonicalize(&path).map_err(|_| Invalid::OutsideObserved)?;
    if observed.iter().any(|root| canonical.starts_with(root)) {
        Ok(canonical)
    } else {
        Err(Invalid::OutsideObserved)
    }
}

/// A ref from a client: `check-ref-format` rules, never an option.
pub fn ref_name(raw: &str) -> Result<RefName, Invalid> {
    RefName::new(raw).map_err(|_| Invalid::InvalidRef)
}

/// Names an agent may not declare: they would pass for a detected agent or
/// for GitRaptor itself (ADR-GRP-005 § 6.6, M7).
const RESERVED_AGENT_NAMES: &[&str] = &["claude code", "claude", "gitraptor", "raptor"];

/// Longest declared agent name, in characters.
pub const MAX_AGENT_NAME_CHARS: usize = 64;

/// A declared agent name (US-GRP-009 registers with it): printable, bounded
/// and not reserved.
pub fn declared_agent_name(raw: &str) -> Result<String, Invalid> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(Invalid::Empty);
    }
    if name.chars().count() > MAX_AGENT_NAME_CHARS {
        return Err(Invalid::TooLong);
    }
    if name.chars().any(|c| {
        c.is_control() || gitraptor_api::untrusted::sanitize(&c.to_string()) != c.to_string()
    }) {
        return Err(Invalid::ControlCharacter);
    }
    let folded: String = name
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if RESERVED_AGENT_NAMES.contains(&folded.as_str()) {
        return Err(Invalid::ReservedName);
    }
    Ok(name.to_owned())
}

/// Whether `path` is inside `root` (both canonical).
pub fn is_within(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unc_device_and_streams_are_refused_lexically() {
        for (raw, why) in [
            (r"\\attacker\share\repo", Invalid::UncOrDevice),
            ("//attacker/share/repo", Invalid::UncOrDevice),
            (r"\\?\C:\repo", Invalid::UncOrDevice),
            (r"\\.\pipe\x", Invalid::UncOrDevice),
            (r"\??\C:\repo", Invalid::UncOrDevice),
            (r"C:\work\NUL", Invalid::DeviceName),
            (r"C:\work\con.txt\x", Invalid::DeviceName),
            (r"C:\work\file.txt:secret", Invalid::AlternateStream),
            ("relative/path", Invalid::NotAbsolute),
            ("/tmp/\u{1b}]52;c;x\u{7}", Invalid::ControlCharacter),
            ("", Invalid::Empty),
        ] {
            assert_eq!(client_path(raw), Err(why), "{raw}");
        }
        assert!(client_path("/Users/u/work/repo").is_ok());
        #[cfg(unix)]
        assert!(client_path("/Users/u/paper/aux").is_ok());
        assert_eq!(client_path(&"/a".repeat(3000)), Err(Invalid::TooLong));
    }

    #[test]
    fn paths_must_resolve_inside_an_observed_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let repo = root.join("repo");
        let outside = root.join("outside");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let observed = vec![repo.clone()];
        assert!(observed_path(repo.join("src").to_str().unwrap(), &observed).is_ok());
        let traversal = format!("{}/src/../../outside", repo.display());
        assert_eq!(
            observed_path(&traversal, &observed),
            Err(Invalid::OutsideObserved)
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, repo.join("escape")).unwrap();
            let link = format!("{}/escape", repo.display());
            assert_eq!(
                observed_path(&link, &observed),
                Err(Invalid::OutsideObserved)
            );
        }
    }

    #[test]
    fn option_like_and_malformed_refs_are_refused() {
        for bad in [
            "--upload-pack=x",
            "-x",
            "a..b",
            "a b",
            "a~1",
            "@",
            "x.lock",
            "",
        ] {
            assert_eq!(ref_name(bad).map(|_| ()), Err(Invalid::InvalidRef), "{bad}");
        }
        assert!(ref_name("feat/login").is_ok());
    }

    #[test]
    fn declared_names_are_bounded_and_not_reserved() {
        assert_eq!(declared_agent_name("Codex").unwrap(), "Codex");
        for reserved in ["Claude Code", "claude   code", "GitRaptor", " raptor "] {
            assert_eq!(declared_agent_name(reserved), Err(Invalid::ReservedName));
        }
        assert_eq!(
            declared_agent_name("a\u{1b}[31m"),
            Err(Invalid::ControlCharacter)
        );
        assert_eq!(
            declared_agent_name("x\u{202e}y"),
            Err(Invalid::ControlCharacter)
        );
        assert_eq!(declared_agent_name(&"x".repeat(65)), Err(Invalid::TooLong));
        assert_eq!(declared_agent_name("  "), Err(Invalid::Empty));
    }
}
