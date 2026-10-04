//! Resolution of the system Git (ADR-GRP-009 § 4, SEC-10, Q28). Fake executables leave a
//! marker when launched, so "not launched" is observable.
#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};

use common::script;
use gitraptor_git::resolve::{
    self, CandidateSource, Rejection, Resolution, ResolveConfig, check_executable,
};
use gitraptor_git::{GitVersion, Invoker};

struct Bed {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl Bed {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        Self { _tmp: tmp, root }
    }

    /// A fake `git` in its own directory that reports `version` and leaves a marker.
    fn fake(&self, dir: &str, version: &str) -> PathBuf {
        let d = self.root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        let git = d.join("git");
        script(
            &git,
            &format!(
                ": > '{}'\necho 'git version {version}'",
                self.marker(dir).display()
            ),
        );
        git
    }

    fn marker(&self, dir: &str) -> PathBuf {
        self.root.join(format!("{dir}.launched"))
    }

    fn config(&self) -> ResolveConfig {
        ResolveConfig {
            configured_path: None,
            path_env: None,
            known_locations: vec![],
            shim_paths: vec![],
            toolchain_gits: vec![],
        }
    }
}

fn invoker() -> Invoker {
    Invoker::default().with_parent_env([("PATH", "/usr/bin:/bin")])
}

fn found(r: &Resolution) -> &Path {
    match r {
        Resolution::Found { git, .. } => &git.path,
        Resolution::NotFound { diagnostics } => panic!("not found: {diagnostics:?}"),
    }
}

#[test]
fn minimal_path_finds_known_location() {
    let bed = Bed::new();
    let git = bed.fake("known", "2.45.1");
    let mut config = bed.config();
    config.path_env = Some("/nonexistent-dir".into());
    config.known_locations = vec![bed.root.join("missing/git"), git.clone()];
    let r = resolve::resolve(&config, &invoker());
    assert_eq!(found(&r), git);
}

#[test]
fn real_machine_git_resolves_with_minimal_path() {
    // Same as launchd/systemd: a minimal PATH, Git found in a well-known location.
    let mut config = ResolveConfig::for_current_os(None);
    config.path_env = Some("/nonexistent-dir".into());
    let r = resolve::resolve(&config, &invoker());
    let Resolution::Found { git, .. } = r else {
        panic!("{r:?}")
    };
    assert!(git.version >= resolve::MIN_VERSION);
}

#[test]
fn path_env_candidates_are_used_in_order() {
    let bed = Bed::new();
    let first = bed.fake("first", "2.39.0");
    let _second = bed.fake("second", "2.50.0");
    let mut config = bed.config();
    config.path_env =
        Some(std::env::join_paths([bed.root.join("first"), bed.root.join("second")]).unwrap());
    assert_eq!(found(&resolve::resolve(&config, &invoker())), first);
    assert!(bed.marker("first").exists(), "the marker must work");
    assert!(!bed.marker("second").exists());
}

#[test]
fn macos_shim_not_launched_without_toolchain() {
    let bed = Bed::new();
    let shim = bed.fake("shim", "2.39.0");
    let mut config = bed.config();
    config.known_locations = vec![shim.clone()];
    config.shim_paths = vec![shim.clone()];
    config.toolchain_gits = vec![bed.root.join("CommandLineTools/usr/bin/git")];
    let r = resolve::resolve(&config, &invoker());
    let Resolution::NotFound { diagnostics } = r else {
        panic!("shim selected")
    };
    assert_eq!(
        diagnostics[0].rejection,
        Rejection::MacosShimWithoutToolchain
    );
    assert!(!bed.marker("shim").exists(), "the shim was launched");

    // With a developer toolchain on disk the shim is a regular candidate.
    let toolchain = bed.fake("CommandLineTools/usr/bin", "2.39.0");
    config.toolchain_gits = vec![toolchain];
    assert_eq!(found(&resolve::resolve(&config, &invoker())), shim);
}

#[test]
fn old_git_reported_insufficient() {
    let bed = Bed::new();
    let old = bed.fake("old", "2.37.1");
    let mut config = bed.config();
    config.known_locations = vec![old];
    let r = resolve::resolve(&config, &invoker());
    assert!(matches!(r, Resolution::NotFound { .. }), "{r:?}");
    assert_eq!(
        r.newest_too_old(),
        Some(GitVersion {
            major: 2,
            minor: 37,
            patch: 1
        })
    );
}

#[test]
fn invalid_git_path_diagnosed_and_resolution_continues() {
    let bed = Bed::new();
    let good = bed.fake("good", "2.38.0");
    for bad in [
        PathBuf::from("relative/git"),
        bed.root.join("missing/git"),
        bed.root.clone(),
    ] {
        let mut config = bed.config();
        config.configured_path = Some(bad.clone());
        config.known_locations = vec![good.clone()];
        let r = resolve::resolve(&config, &invoker());
        let Resolution::Found { git, diagnostics } = r else {
            panic!("{bad:?}")
        };
        assert_eq!(git.path, good);
        assert_eq!(diagnostics.len(), 1, "{bad:?}");
        assert_eq!(diagnostics[0].source, CandidateSource::ConfiguredPath);
        assert_eq!(diagnostics[0].path, bad);
    }
}

#[test]
fn relative_git_path_rejected() {
    assert_eq!(
        check_executable(Path::new("bin/git")),
        Err(Rejection::NotAbsolute)
    );
}

#[test]
fn world_writable_git_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let bed = Bed::new();
    for mode in [0o777, 0o775, 0o757] {
        let git = bed.fake("writable", "2.40.0");
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(mode)).unwrap();
        let mut config = bed.config();
        config.configured_path = Some(git.clone());
        let r = resolve::resolve(&config, &invoker());
        let Resolution::NotFound { diagnostics } = r else {
            panic!("mode {mode:o} accepted")
        };
        assert_eq!(diagnostics[0].rejection, Rejection::WritableByOthers);
        assert!(!bed.marker("writable").exists(), "mode {mode:o} launched");
    }
}

#[test]
fn non_executable_and_directory_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let bed = Bed::new();
    let git = bed.fake("noexec", "2.40.0");
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(check_executable(&git), Err(Rejection::NotExecutable));
    assert_eq!(check_executable(&bed.root), Err(Rejection::NotRegularFile));
}

#[test]
fn relative_path_entries_are_ignored() {
    let bed = Bed::new();
    let _planted = bed.fake("planted", "2.40.0");
    let mut config = bed.config();
    // A repository could plant `git` in the cwd or a relative directory.
    config.path_env = Some(".:planted:./planted:".into());
    let r = resolve::resolve(&config, &invoker());
    assert_eq!(
        r,
        Resolution::NotFound {
            diagnostics: vec![]
        }
    );
    assert!(!bed.marker("planted").exists());
}

#[test]
fn symlinked_git_resolves_to_its_target() {
    let bed = Bed::new();
    let real = bed.fake("cellar", "2.41.0");
    let link_dir = bed.root.join("links");
    std::fs::create_dir_all(&link_dir).unwrap();
    std::os::unix::fs::symlink(&real, link_dir.join("git")).unwrap();
    let mut config = bed.config();
    config.path_env = Some(link_dir.into_os_string());
    assert_eq!(found(&resolve::resolve(&config, &invoker())), real);
}
