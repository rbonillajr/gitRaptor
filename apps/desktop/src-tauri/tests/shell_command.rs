//! D8-SHELL (DS-US-DSK-008 D11, D21): the commands the app offers to paste. `posix` quotes every
//! argument with single quotes (a quote is `'\''`) and refuses text that acts on a terminal;
//! `raptor_path` names the daemon's own `raptor` unless it hangs from a watched path, then the
//! bundled one, never `PATH`; `confirm` builds `raptor guard confirm <repo>`.
//! The "installed" `raptor` files live under `CARGO_TARGET_TMPDIR` (not under a system temp
//! folder, which `is_installed_raptor` rejects). Temporary folders only (NFR-01).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use gitraptor_desktop::engine::shell_command::{confirm, posix, raptor_path};
use gitraptor_desktop::engine::wire::{ConfirmCommandDto, NotOfferedReason};

/// A `raptor` file (0755) in a fresh folder of the target dir; returns the folder guard and the
/// canonical path of the file.
fn installed_raptor(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix(name)
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("temporary folder");
    let file = dir.path().join("raptor");
    std::fs::write(&file, "#!/bin/sh\n").expect("write raptor");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let canonical = file.canonicalize().expect("canonical raptor");
    (dir, canonical)
}

#[test]
fn the_confirm_command_quotes_the_daemon_raptor_and_the_raw_repo_path() {
    let got = confirm(
        Some(Path::new("/opt/git raptor/raptor")),
        "/w/o'brien/tienda",
    );
    assert_eq!(
        got,
        ConfirmCommandDto::Offered {
            command: "'/opt/git raptor/raptor' 'guard' 'confirm' '/w/o'\\''brien/tienda'"
                .to_owned(),
            with_path: true,
        },
        "D8-SHELL: the command is raptor guard confirm <raw repo path>, every argument single-quoted"
    );
}

#[test]
fn a_repo_path_with_unsafe_text_gives_the_command_without_path() {
    for c in ['\u{1b}', '\u{7}', '\n', '\u{202e}', '\u{200b}'] {
        let got = confirm(
            Some(Path::new("/opt/gitraptor/raptor")),
            &format!("/w/{c}/tienda"),
        );
        assert_eq!(
            got,
            ConfirmCommandDto::Offered {
                command: "'/opt/gitraptor/raptor' 'guard' 'confirm'".to_owned(),
                with_path: false,
            },
            "D8-SHELL: a repo path with {c:?} must give the command without the path"
        );
    }
}

#[test]
fn without_a_known_raptor_the_confirm_command_is_not_offered() {
    assert_eq!(
        confirm(None, "/w/tienda"),
        ConfirmCommandDto::NotOffered {
            reason: NotOfferedReason::RaptorUnknown
        },
        "D8-SHELL: no raptor known means no command, never a bare `raptor`"
    );
    assert!(
        matches!(
            confirm(Some(Path::new("/opt/gitraptor/raptor")), "/w/tienda"),
            ConfirmCommandDto::Offered { .. }
        ),
        "D8-SHELL: the same repo with a known raptor does get its command"
    );
}

#[test]
fn a_quote_is_written_as_posix_escape() {
    assert_eq!(
        posix(&["it's", "a b", "$HOME `id`"]).as_deref(),
        Some("'it'\\''s' 'a b' '$HOME `id`'"),
        "D8-SHELL: a quote is written '\\'' and metacharacters stay inert inside single quotes"
    );
    assert_eq!(
        posix(&["ok", "bad\u{1b}]52;c;x\u{7}"]),
        None,
        "D8-SHELL: an argument that acts on the terminal gives no command"
    );
}

#[test]
fn the_daemon_raptor_of_the_ready_report_is_used() {
    let (_daemon_dir, daemon) = installed_raptor("daemon-");
    let (_bundle_dir, bundled) = installed_raptor("bundle-");
    assert_eq!(
        raptor_path(Some(&daemon), Some(&bundled), &[]),
        Some(daemon),
        "D8-SHELL: the raptor that serves the daemon wins over the bundled one"
    );
}

#[test]
fn a_daemon_raptor_inside_a_watched_worktree_falls_back_to_the_bundle() {
    let (worktree, daemon) = installed_raptor("worktree-");
    let (_bundle_dir, bundled) = installed_raptor("bundle-");
    let watched = worktree.path().canonicalize().expect("canonical worktree");
    assert_eq!(
        raptor_path(Some(&daemon), Some(&bundled), &[watched.as_path()]),
        Some(bundled.clone()),
        "D8-SHELL: an agent could leave its own raptor in a watched worktree (L-02)"
    );
    assert_eq!(
        raptor_path(Some(&daemon), None, &[watched.as_path()]),
        None,
        "D8-SHELL: with no bundled raptor either, nothing is offered"
    );
}

#[test]
fn never_path() {
    assert_eq!(
        raptor_path(None, None, &[]),
        None,
        "D8-SHELL: without a daemon raptor or a bundled one there is no raptor, never PATH"
    );
    assert_eq!(
        raptor_path(Some(Path::new("/nonexistent/dir/raptor")), None, &[]),
        None,
        "D8-SHELL: a daemon exe that is not an installed raptor is not named"
    );
    let (_bundle_dir, bundled) = installed_raptor("bundle-");
    assert_eq!(
        raptor_path(
            Some(Path::new("/nonexistent/dir/raptor")),
            Some(&bundled),
            &[]
        ),
        Some(bundled),
        "D8-SHELL: a daemon exe that is not an installed raptor falls back to the bundle"
    );
}
