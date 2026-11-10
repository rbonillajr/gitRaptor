//! TS-GRP-009 (DS-TS-GRP-009 § 9.4, file 2): the path identity of the desktop app's executable
//! (ADR-GRP-017 § 3, row 3, D4). A pure function over a read-only file system port: a `MemFs`
//! double fabricates the trees, and one test reads the real modes of a temporary directory.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use gitraptor_core::channel::desktop::{AppFs, FsMeta, PathIdentity, path_identity};

/// The uid of the daemon in these tests.
const UID: u32 = 501;
const APP_EXE: &str = "/Applications/GitRaptor.app/Contents/MacOS/GitRaptor";

/// A file system in memory: what `metadata` and `canonicalize` answer, plus the paths that fail.
#[derive(Default)]
struct MemFs {
    meta: HashMap<PathBuf, FsMeta>,
    links: HashMap<PathBuf, PathBuf>,
    canonicalize_fails: HashSet<PathBuf>,
    metadata_fails: HashSet<PathBuf>,
}

impl AppFs for MemFs {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        if self.canonicalize_fails.contains(path) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        if let Some(target) = self.links.get(path) {
            return Ok(target.clone());
        }
        if self.meta.contains_key(path) {
            Ok(path.to_path_buf())
        } else {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }

    fn metadata(&self, path: &Path) -> io::Result<FsMeta> {
        if self.metadata_fails.contains(path) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        self.meta
            .get(path)
            .copied()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
}

impl MemFs {
    /// The executable `exe` and every folder above it: `0755`, owned by the daemon's user, device 1.
    fn with_exe(mut self, exe: &str) -> Self {
        let exe = Path::new(exe);
        self.meta
            .insert(exe.to_path_buf(), meta(false, 0o755, UID, 1));
        for dir in exe.ancestors().skip(1) {
            self.meta
                .insert(dir.to_path_buf(), meta(true, 0o755, UID, 1));
        }
        self
    }

    /// The system folder of applications as macOS has it: `root:admin 0775`.
    fn system_apps(mut self) -> Self {
        self.meta
            .insert(PathBuf::from("/Applications"), meta(true, 0o775, 0, 1));
        self
    }

    fn mode(mut self, path: &str, mode: u32) -> Self {
        self.meta.get_mut(Path::new(path)).unwrap().mode = mode;
        self
    }

    fn owner(mut self, path: &str, uid: u32) -> Self {
        self.meta.get_mut(Path::new(path)).unwrap().uid = uid;
        self
    }

    fn device(mut self, path: &str, dev: u64) -> Self {
        self.meta.get_mut(Path::new(path)).unwrap().dev = dev;
        self
    }
}

fn meta(is_dir: bool, mode: u32, uid: u32, dev: u64) -> FsMeta {
    FsMeta {
        is_file: !is_dir,
        is_dir,
        mode,
        uid,
        dev,
    }
}

fn apps() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/Users/dev/Applications"),
    ]
}

fn identity(exe: &str, fs: &MemFs) -> PathIdentity {
    path_identity(Some(Path::new(exe)), &apps(), fs, UID)
}

fn identity_in(exe: &str, folders: &[&str], fs: &MemFs) -> PathIdentity {
    let folders: Vec<PathBuf> = folders.iter().map(PathBuf::from).collect();
    path_identity(Some(Path::new(exe)), &folders, fs, UID)
}

/// The baseline every rejection starts from: the app as the system folder installs it.
fn installed() -> MemFs {
    MemFs::default().with_exe(APP_EXE).system_apps()
}

fn assert_identity(got: PathIdentity, want: PathIdentity, why: &str) {
    assert_eq!(got, want, "{why}");
}

#[test]
fn app_in_system_applications_is_verified() {
    assert_identity(
        identity(APP_EXE, &installed()),
        PathIdentity::Verified,
        "GitRaptor.app/Contents/MacOS/GitRaptor in /Applications must be verified",
    );
    // The folder and the bundle may belong to root as well as to the daemon's user.
    let root_owned = installed()
        .owner(APP_EXE, 0)
        .owner("/Applications/GitRaptor.app/Contents/MacOS", 0)
        .owner("/Applications/GitRaptor.app/Contents", 0)
        .owner("/Applications/GitRaptor.app", 0);
    assert_identity(
        identity(APP_EXE, &root_owned),
        PathIdentity::Verified,
        "a bundle owned by root (uid 0) must be verified",
    );
}

#[test]
fn app_in_user_applications_is_verified() {
    let exe = "/Users/dev/Applications/GitRaptor.app/Contents/MacOS/GitRaptor";
    assert_identity(
        identity(exe, &MemFs::default().with_exe(exe)),
        PathIdentity::Verified,
        "the app in the Applications folder of the user's home must be verified",
    );
}

/// A folder of applications that does not exist (or cannot be read) is skipped: it does not
/// block the one that holds the app.
#[test]
fn missing_user_applications_folder_does_not_block_system_applications() {
    let folders = ["/Applications", "/Users/u/Applications"];
    let fs = installed();
    assert!(
        fs.metadata(Path::new("/Users/u/Applications")).is_err()
            && fs.canonicalize(Path::new("/Users/u/Applications")).is_err(),
        "fixture: the user's applications folder must be missing from the double"
    );
    assert_identity(
        identity_in(APP_EXE, &folders, &fs),
        PathIdentity::Verified,
        "a missing ~/Applications must be skipped: the app in /Applications must be verified",
    );
    // An unreadable folder (error other than not found) is skipped too.
    let mut unreadable = installed();
    unreadable
        .metadata_fails
        .insert(PathBuf::from("/Users/u/Applications"));
    unreadable
        .canonicalize_fails
        .insert(PathBuf::from("/Users/u/Applications"));
    assert_identity(
        identity_in(APP_EXE, &folders, &unreadable),
        PathIdentity::Verified,
        "an unreadable ~/Applications must be skipped: the app in /Applications must be verified",
    );
}

#[test]
fn both_app_binary_names_are_accepted() {
    for name in ["GitRaptor", "gitraptor-desktop"] {
        let exe = format!("/Applications/GitRaptor.app/Contents/MacOS/{name}");
        assert_identity(
            identity(&exe, &MemFs::default().with_exe(&exe).system_apps()),
            PathIdentity::Verified,
            &format!("the binary `{name}` of the app must be verified"),
        );
    }
}

#[test]
fn bundled_raptor_and_raptor_mcp_are_not_the_app() {
    for name in ["raptor", "raptor-mcp"] {
        let exe = format!("/Applications/GitRaptor.app/Contents/MacOS/{name}");
        assert_identity(
            identity(&exe, &MemFs::default().with_exe(&exe).system_apps()),
            PathIdentity::Absent,
            &format!("the bundled `{name}` lives in the same folder but is not the app"),
        );
    }
}

#[test]
fn other_name_is_rejected() {
    for name in ["gitraptor", "GitRaptor2", "GitRaptor-desktop", "Git"] {
        let exe = format!("/Applications/GitRaptor.app/Contents/MacOS/{name}");
        assert_identity(
            identity(&exe, &MemFs::default().with_exe(&exe).system_apps()),
            PathIdentity::Absent,
            &format!("the name `{name}` is not one of the app's binaries"),
        );
    }
}

#[test]
fn binary_outside_applications_is_rejected() {
    for exe in [
        "/Users/dev/Downloads/GitRaptor.app/Contents/MacOS/GitRaptor",
        "/opt/GitRaptor.app/Contents/MacOS/GitRaptor",
        "/tmp/GitRaptor.app/Contents/MacOS/gitraptor-desktop",
    ] {
        assert_identity(
            identity(exe, &MemFs::default().with_exe(exe).system_apps()),
            PathIdentity::Absent,
            &format!("{exe} is outside the applications folders and must be rejected"),
        );
    }
}

#[test]
fn nested_or_renamed_bundle_is_rejected() {
    for exe in [
        "/Applications/x/GitRaptor.app/Contents/MacOS/GitRaptor",
        "/Applications/Other.app/Contents/MacOS/GitRaptor",
        "/Applications/GitRaptor.app/Contents/Other/GitRaptor",
        "/Applications/GitRaptor.app/Contents/MacOS/x/GitRaptor",
    ] {
        assert_identity(
            identity(exe, &MemFs::default().with_exe(exe).system_apps()),
            PathIdentity::Absent,
            &format!("{exe} is nested or its bundle is renamed and must be rejected"),
        );
    }
}

/// A translocated copy runs from a read-only mount the system makes of a downloaded app: it never
/// proves where the app was installed, even if the folder it sits in is one the daemon trusts.
#[test]
fn translocated_path_is_rejected() {
    let folder = "/private/var/folders/zz/AppTranslocation/0A1B2C3D/d";
    let exe = format!("{folder}/GitRaptor.app/Contents/MacOS/GitRaptor");
    let fs = MemFs::default().with_exe(&exe);
    assert_identity(
        identity_in(&exe, &[folder], &fs),
        PathIdentity::Absent,
        "a path with an AppTranslocation component must be rejected",
    );
}

#[test]
fn mounted_volume_is_rejected() {
    let folder = "/Volumes/GitRaptor";
    let exe = format!("{folder}/GitRaptor.app/Contents/MacOS/GitRaptor");
    let fs = MemFs::default().with_exe(&exe);
    assert_identity(
        identity_in(&exe, &[folder], &fs),
        PathIdentity::Absent,
        "an app run from a mounted volume (/Volumes) must be rejected",
    );
}

#[test]
fn other_device_than_applications_folder_is_rejected() {
    let mut fs = installed();
    for path in [
        APP_EXE,
        "/Applications/GitRaptor.app/Contents/MacOS",
        "/Applications/GitRaptor.app/Contents",
        "/Applications/GitRaptor.app",
    ] {
        fs = fs.device(path, 2);
    }
    assert_identity(
        identity(APP_EXE, &fs),
        PathIdentity::Absent,
        "a bundle on another device than the applications folder must be rejected",
    );
}

#[test]
fn symlink_is_resolved_before_checking() {
    let target = "/Users/dev/Downloads/GitRaptor.app/Contents/MacOS/GitRaptor";
    let mut fs = MemFs::default()
        .with_exe(APP_EXE)
        .with_exe(target)
        .system_apps();
    fs.links
        .insert(PathBuf::from(APP_EXE), PathBuf::from(target));
    assert_identity(
        identity(APP_EXE, &fs),
        PathIdentity::Absent,
        "a link in /Applications to ~/Downloads must be judged by its canonical target",
    );
}

#[test]
fn writable_executable_is_rejected() {
    for mode in [0o775, 0o757, 0o777, 0o722] {
        assert_identity(
            identity(APP_EXE, &installed().mode(APP_EXE, mode)),
            PathIdentity::Absent,
            &format!("an executable with mode {mode:o} (group or others write) must be rejected"),
        );
    }
}

#[test]
fn writable_bundle_directory_is_rejected() {
    for dir in [
        "/Applications/GitRaptor.app",
        "/Applications/GitRaptor.app/Contents",
        "/Applications/GitRaptor.app/Contents/MacOS",
    ] {
        for mode in [0o775, 0o757] {
            assert_identity(
                identity(APP_EXE, &installed().mode(dir, mode)),
                PathIdentity::Absent,
                &format!("{dir} with mode {mode:o} (group or others write) must be rejected"),
            );
        }
    }
}

#[test]
fn group_writable_system_applications_is_accepted() {
    assert_identity(
        identity(APP_EXE, &installed().mode("/Applications", 0o775)),
        PathIdentity::Verified,
        "/Applications as macOS has it (root:admin 0775) must not reject the app",
    );
}

#[test]
fn world_writable_applications_folder_is_rejected() {
    for mode in [0o777, 0o757] {
        assert_identity(
            identity(APP_EXE, &installed().mode("/Applications", mode)),
            PathIdentity::Absent,
            &format!("an applications folder with mode {mode:o} (others write) must be rejected"),
        );
    }
}

#[test]
fn foreign_owner_is_rejected() {
    for path in [
        APP_EXE,
        "/Applications/GitRaptor.app/Contents/MacOS",
        "/Applications/GitRaptor.app/Contents",
        "/Applications/GitRaptor.app",
    ] {
        assert_identity(
            identity(APP_EXE, &installed().owner(path, 502)),
            PathIdentity::Absent,
            &format!("{path} owned by another user (not the daemon's, not root) must be rejected"),
        );
    }
}

#[test]
fn unreadable_executable_is_unverifiable() {
    let exe = Path::new(APP_EXE);
    // The same tree, readable, is verified: what follows is a failure to read, not a rejection.
    assert_identity(
        identity(APP_EXE, &installed()),
        PathIdentity::Verified,
        "control: the readable app must be verified before the unreadable one is judged",
    );

    let mut canonicalize_fails = installed();
    canonicalize_fails
        .canonicalize_fails
        .insert(exe.to_path_buf());
    assert_identity(
        identity(APP_EXE, &canonicalize_fails),
        PathIdentity::Unverifiable,
        "an executable whose canonicalization fails must be unverifiable",
    );

    let mut metadata_fails = installed();
    metadata_fails.metadata_fails.insert(exe.to_path_buf());
    assert_identity(
        identity(APP_EXE, &metadata_fails),
        PathIdentity::Unverifiable,
        "an executable whose metadata cannot be read must be unverifiable",
    );

    assert_identity(
        path_identity(None, &apps(), &installed(), UID),
        PathIdentity::Unverifiable,
        "a peer without an executable path must be unverifiable",
    );
}

/// The real `SystemFs`: modes and owner come from the file system, not from a double.
#[cfg(unix)]
#[test]
fn real_filesystem_modes_are_read() {
    use gitraptor_core::channel::desktop::SystemFs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let applications = root.join("Applications");
    let macos = applications.join("GitRaptor.app/Contents/MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    let exe = macos.join("GitRaptor");
    std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
    let chmod = |path: &Path, mode: u32| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    chmod(&applications, 0o755);
    for dir in [
        applications.join("GitRaptor.app"),
        applications.join("GitRaptor.app/Contents"),
        macos.clone(),
    ] {
        chmod(&dir, 0o755);
    }
    chmod(&exe, 0o755);
    let uid = std::fs::metadata(&exe).unwrap().uid();
    let folders = [applications.clone()];

    assert_eq!(
        path_identity(Some(&exe), &folders, &SystemFs, uid),
        PathIdentity::Verified,
        "the real 0755 bundle in a temporary applications folder must be verified"
    );

    chmod(&macos, 0o775);
    assert_eq!(
        path_identity(Some(&exe), &folders, &SystemFs, uid),
        PathIdentity::Absent,
        "after chmod 0775 on Contents/MacOS the real bundle must be rejected"
    );
}

/// The folders come from the user database for the daemon's uid, never from the environment.
#[cfg(target_os = "macos")]
#[test]
fn app_dirs_come_from_the_user_database() {
    use gitraptor_core::channel::desktop::system_app_dirs;

    let uid = nix::unistd::geteuid();
    let user = nix::unistd::User::from_uid(uid)
        .unwrap()
        .expect("the test's user has an entry in the user database");
    assert_eq!(
        system_app_dirs(uid.as_raw()),
        vec![
            PathBuf::from("/Applications"),
            user.dir.join("Applications")
        ],
        "system_app_dirs must be /Applications and <pw_dir>/Applications of the uid"
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn app_dirs_come_from_the_user_database() {
    use gitraptor_core::channel::desktop::system_app_dirs;

    assert!(
        system_app_dirs(1000).is_empty(),
        "outside macOS there are no applications folders (identity is always none)"
    );
}
