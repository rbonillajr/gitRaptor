//! Resolution and verification of the system Git (ADR-GRP-009 § 4, SEC-10, Q28, NFR-07).
//!
//! Candidates are checked on the file system before anything runs. Only a candidate that passes
//! is asked for `git version`, and it is selected if it reports 2.38 or later.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::invoke::{Invoker, Subcommand};

/// Minimum supported Git version (NFR-07).
pub const MIN_VERSION: GitVersion = GitVersion {
    major: 2,
    minor: 38,
    patch: 0,
};

/// A Git version as reported by `git version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl std::fmt::Display for GitVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl GitVersion {
    /// Parse the output of `git version`, e.g. `git version 2.50.1 (Apple Git-155)` or
    /// `git version 2.45.1.windows.1`.
    pub fn parse(output: &str) -> Option<Self> {
        let rest = output.trim().strip_prefix("git version ")?;
        let mut numbers = rest
            .split(|c: char| !c.is_ascii_digit())
            .take_while(|s| !s.is_empty());
        let major = numbers.next()?.parse().ok()?;
        let minor = numbers.next()?.parse().ok()?;
        let patch = numbers.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

/// A Git executable that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemGit {
    /// Canonical absolute path of the executable.
    pub path: PathBuf,
    /// Reported version, at least [`MIN_VERSION`].
    pub version: GitVersion,
}

/// Where a candidate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    /// `engine.gitPath` of the profile configuration (ADR-GRP-007).
    ConfiguredPath,
    /// An entry of the inherited `PATH`.
    PathEnv,
    /// A well-known location for the OS.
    KnownLocation,
}

/// Why a candidate was not selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// No file at that path.
    Missing,
    /// The path is relative.
    NotAbsolute,
    /// Not a regular file.
    NotRegularFile,
    /// Not owned by the current user or by root.
    UntrustedOwner,
    /// Writable by group or others.
    WritableByOthers,
    /// No execute permission.
    NotExecutable,
    /// Windows: the ACL could not be read or understood, or the path is not on a local volume
    /// or goes through a reparse point (fail-closed).
    AclUnverified,
    /// macOS `/usr/bin/git` shim without a developer toolchain; it was not launched.
    MacosShimWithoutToolchain,
    /// `git version` failed, timed out or printed something unexpected.
    NoVersion(String),
    /// The version is older than [`MIN_VERSION`].
    TooOld(GitVersion),
}

/// One candidate and why it was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub path: PathBuf,
    pub source: CandidateSource,
    pub rejection: Rejection,
}

/// Outcome of [`resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A valid Git was found. `diagnostics` lists the candidates rejected before it.
    Found {
        git: SystemGit,
        diagnostics: Vec<Diagnostic>,
    },
    /// No candidate qualifies; `diagnostics` says why (absent or too old).
    NotFound { diagnostics: Vec<Diagnostic> },
}

impl Resolution {
    /// Highest version among the rejected-as-too-old candidates, to report "Git too old".
    pub fn newest_too_old(&self) -> Option<GitVersion> {
        let diagnostics = match self {
            Self::Found { diagnostics, .. } | Self::NotFound { diagnostics } => diagnostics,
        };
        diagnostics
            .iter()
            .filter_map(|d| match d.rejection {
                Rejection::TooOld(v) => Some(v),
                _ => None,
            })
            .max()
    }
}

/// Inputs of the resolution. [`ResolveConfig::for_current_os`] fills them for the running OS;
/// tests replace the locations with fakes.
#[derive(Debug, Clone)]
pub struct ResolveConfig {
    /// `engine.gitPath` from the profile, if set.
    pub configured_path: Option<PathBuf>,
    /// The inherited `PATH`.
    pub path_env: Option<OsString>,
    /// Well-known locations, in order.
    pub known_locations: Vec<PathBuf>,
    /// macOS `/usr/bin/git` shims: launched only if a developer toolchain exists.
    pub shim_paths: Vec<PathBuf>,
    /// Git binaries of a developer toolchain (Command Line Tools, Xcode).
    pub toolchain_gits: Vec<PathBuf>,
}

impl ResolveConfig {
    /// Defaults for the running OS, with `configured_path` and the process `PATH`.
    pub fn for_current_os(configured_path: Option<PathBuf>) -> Self {
        let mut config = Self {
            configured_path,
            path_env: std::env::var_os("PATH"),
            known_locations: Vec::new(),
            shim_paths: Vec::new(),
            toolchain_gits: Vec::new(),
        };
        if cfg!(target_os = "macos") {
            config.toolchain_gits = vec![
                "/Library/Developer/CommandLineTools/usr/bin/git".into(),
                "/Applications/Xcode.app/Contents/Developer/usr/bin/git".into(),
            ];
            // The developer directory selected with `xcode-select`, read from the FS.
            if let Ok(dir) = std::fs::read_link("/var/db/xcode_select_link") {
                config.toolchain_gits.push(dir.join("usr/bin/git"));
            }
            config.shim_paths = vec!["/usr/bin/git".into()];
            config.known_locations =
                vec!["/opt/homebrew/bin/git".into(), "/usr/local/bin/git".into()];
            config
                .known_locations
                .extend(config.toolchain_gits.iter().cloned());
        } else if cfg!(windows) {
            for (var, suffix) in [
                ("ProgramFiles", r"Git\cmd\git.exe"),
                ("LOCALAPPDATA", r"Programs\Git\cmd\git.exe"),
                ("USERPROFILE", r"scoop\shims\git.exe"),
            ] {
                if let Some(base) = std::env::var_os(var) {
                    config
                        .known_locations
                        .push(PathBuf::from(base).join(suffix));
                }
            }
        } else {
            config.known_locations = vec!["/usr/bin/git".into(), "/usr/local/bin/git".into()];
            if let Some(home) = std::env::var_os("HOME") {
                config
                    .known_locations
                    .push(PathBuf::from(home).join(".nix-profile/bin/git"));
            }
        }
        config
    }
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "git.exe" } else { "git" }
}

/// Find the first valid Git of at least [`MIN_VERSION`].
pub fn resolve(config: &ResolveConfig, invoker: &Invoker) -> Resolution {
    let mut candidates: Vec<(PathBuf, CandidateSource)> = Vec::new();
    if let Some(p) = &config.configured_path {
        candidates.push((p.clone(), CandidateSource::ConfiguredPath));
    }
    if let Some(path) = &config.path_env {
        // Relative entries are ignored: a repository could plant a `git` in the cwd.
        candidates.extend(
            std::env::split_paths(path)
                .filter(|dir| dir.is_absolute())
                .map(|dir| (dir.join(exe_name()), CandidateSource::PathEnv)),
        );
    }
    candidates.extend(
        config
            .known_locations
            .iter()
            .map(|p| (p.clone(), CandidateSource::KnownLocation)),
    );

    let mut diagnostics = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for (path, source) in candidates {
        // A missing PATH entry or well-known location is not worth a diagnostic, an explicit
        // `gitPath` always is.
        let reject = |rejection| Diagnostic {
            path: path.clone(),
            source,
            rejection,
        };
        let canonical = match check_executable(&path) {
            Ok(c) => c,
            Err(Rejection::Missing) if source != CandidateSource::ConfiguredPath => continue,
            Err(r) => {
                diagnostics.push(reject(r));
                continue;
            }
        };
        if seen.contains(&canonical) {
            continue;
        }
        seen.push(canonical.clone());
        if config.shim_paths.contains(&canonical)
            && !config.toolchain_gits.iter().any(|g| g.is_file())
        {
            diagnostics.push(reject(Rejection::MacosShimWithoutToolchain));
            continue;
        }
        match query_version(&canonical, invoker) {
            Ok(v) if v >= MIN_VERSION => {
                return Resolution::Found {
                    git: SystemGit {
                        path: canonical,
                        version: v,
                    },
                    diagnostics,
                };
            }
            Ok(v) => diagnostics.push(reject(Rejection::TooOld(v))),
            Err(e) => diagnostics.push(reject(Rejection::NoVersion(e))),
        }
    }
    Resolution::NotFound { diagnostics }
}

fn query_version(git: &Path, invoker: &Invoker) -> Result<GitVersion, String> {
    let stdout = invoker
        .run(git, None, Subcommand::Version, &[])
        .and_then(|o| o.into_success("git version"))
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&stdout);
    GitVersion::parse(&text).ok_or_else(|| "unexpected `git version` output".to_owned())
}

/// Check a candidate without executing it (SEC-10) and return its canonical path.
pub fn check_executable(path: &Path) -> Result<PathBuf, Rejection> {
    if !path.is_absolute() {
        return Err(Rejection::NotAbsolute);
    }
    if crate::paths::validate(path).is_err() {
        return Err(Rejection::NotAbsolute);
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| Rejection::Missing)?;
    let meta = std::fs::symlink_metadata(&canonical).map_err(|_| Rejection::Missing)?;
    if !meta.is_file() {
        return Err(Rejection::NotRegularFile);
    }
    check_owner_and_mode(&canonical, &meta)?;
    Ok(canonical)
}

#[cfg(unix)]
fn check_owner_and_mode(path: &Path, meta: &std::fs::Metadata) -> Result<(), Rejection> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let owned_by_user = gix::sec::identity::is_path_owned_by_current_user(path).unwrap_or(false);
    if !owned_by_user && meta.uid() != 0 {
        return Err(Rejection::UntrustedOwner);
    }
    let mode = meta.permissions().mode();
    if mode & 0o022 != 0 {
        return Err(Rejection::WritableByOthers);
    }
    if mode & 0o111 == 0 {
        return Err(Rejection::NotExecutable);
    }
    Ok(())
}

/// Windows: owner and DACL of the file, its folder and every folder above it (SEC-10,
/// TD-GRP-001). No location is trusted for being under `%ProgramFiles%`.
#[cfg(windows)]
fn check_owner_and_mode(path: &Path, _meta: &std::fs::Metadata) -> Result<(), Rejection> {
    use gitraptor_winsys::acl::{AclError, verify_trusted_executable};
    match verify_trusted_executable(path) {
        Ok(()) => Ok(()),
        Err(AclError::UntrustedOwner(_)) => Err(Rejection::UntrustedOwner),
        Err(AclError::UntrustedWriter(_)) => Err(Rejection::WritableByOthers),
        Err(_) => Err(Rejection::AclUnverified),
    }
}

/// Any other OS: nothing is verified, so nothing is accepted.
#[cfg(not(any(unix, windows)))]
fn check_owner_and_mode(_path: &Path, _meta: &std::fs::Metadata) -> Result<(), Rejection> {
    Err(Rejection::AclUnverified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        let v = |s| GitVersion::parse(s);
        assert_eq!(
            v("git version 2.50.1 (Apple Git-155)\n"),
            Some(GitVersion {
                major: 2,
                minor: 50,
                patch: 1
            })
        );
        assert_eq!(
            v("git version 2.45.1.windows.1"),
            Some(GitVersion {
                major: 2,
                minor: 45,
                patch: 1
            })
        );
        assert_eq!(
            v("git version 2.38"),
            Some(GitVersion {
                major: 2,
                minor: 38,
                patch: 0
            })
        );
        assert_eq!(v("hello"), None);
        assert!(v("git version 2.37.9").unwrap() < MIN_VERSION);
        assert!(v("git version 2.38.0").unwrap() >= MIN_VERSION);
    }
}
