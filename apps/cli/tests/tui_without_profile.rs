//! Suite "TUI sin perfil" of INF-GRP-001 (owner: INF-CKP-001, Entrega 2b): the TUI reads
//! nothing of the profile itself. With the profile's data and configuration folders
//! unreadable, a headless `App` on the real daemon reaches "live" with the same replica,
//! and the fake machine stays byte for byte (only the engine's own data and state, and the
//! times of the configuration folder whose mode the test flips, may change).
//!
//! Temporary profile (`GITRAPTOR_PROFILE_DIR`, debug builds only); never this repo nor the
//! real profile (NFR-01). Linux and Windows: Pendiente: etapa de validación multiplataforma
//! (Windows has no mode bits to drop; the suite is Unix-only).
#![cfg(all(unix, debug_assertions))]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::client::NeverLaunch;
use gitraptor_api::messages::ClientKind;
use gitraptor_cli::client;
use gitraptor_cli::client::engine::EngineConnector;
use gitraptor_cli::model::{ConnState, Model, Size};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::queue;
use gitraptor_cli::tui::app::App;
use gitraptor_core::client::ClientOptions;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn raptor(f: &Fixture, args: &[&str]) -> Output {
    let env: Vec<(&str, OsString)> = vec![
        ("GITRAPTOR_PROFILE_DIR", f.profile.clone().into_os_string()),
        ("HOME", f.home.clone().into_os_string()),
        ("PATH", "/usr/bin:/bin".into()),
        ("LANG", "en_US.UTF-8".into()),
    ];
    Command::new(RAPTOR)
        .args(args)
        .env_clear()
        .envs(env)
        .current_dir(&f.root)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// A headless TUI on the daemon of `dirs`, stepped until it is live and synced.
fn live_tui(dirs: &ProfileDirs, cwd: &Path) -> bool {
    let options = ClientOptions::new(dirs.clone(), ClientKind::Cli);
    let (inbox, _input, engine) = queue::inbox();
    let mut app = App::new(
        Terminal::new(TestBackend::new(80, 24)).unwrap(),
        Model::new(
            Lang::En,
            Size {
                width: 80,
                height: 24,
            },
        ),
        inbox,
    );
    // Never starts one: the daemon of the test is already running.
    let connector = EngineConnector::new(options.connect().unwrap(), Box::new(NeverLaunch));
    let channel = client::spawn(connector, Some(cwd.to_owned()), engine);
    app.attach(channel.cmds.clone());
    let start = Instant::now();
    while !(app.model.conn == ConnState::Live && app.model.engine.all_synced()) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the TUI stayed in {:?}",
            app.model.conn
        );
        app.step(Duration::from_millis(10)).unwrap();
    }
    channel.shutdown();
    app.model.engine.global.data.is_some()
}

mod repo_intact {
    use super::*;

    #[test]
    fn repo_intact_the_tui_works_without_reading_the_profile() {
        let f = Fixture::with_commit(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            set_mode(&f.profile.join(dir), 0o700);
        }
        let dirs = ProfileDirs::under_root(&f.profile);
        // The daemon starts in the temporary profile, outside the checked window.
        let status = raptor(&f, &["daemon", "status"]);
        assert!(status.status.success(), "{status:?}");
        let with_profile = live_tui(&dirs, &f.repo);

        let exceptions = Exceptions::engine_profile("profile").with(Exception::DirTimes {
            scope: "profile".into(),
            path: "config".into(),
        });
        let mut without_profile = false;
        let report = check("INF-CKP-001 TUI sin perfil", &f, &exceptions, || {
            set_mode(&dirs.data, 0o000);
            set_mode(&dirs.config, 0o000);
            let seen = std::panic::catch_unwind(|| live_tui(&dirs, &f.repo));
            set_mode(&dirs.data, 0o700);
            set_mode(&dirs.config, 0o700);
            without_profile = seen.expect("the TUI needs the profile");
        });
        if let Ok(Some(pid)) = gitraptor_core::daemon::running_pid(&dirs.state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
        report.assert_intact();
        assert!(with_profile, "no global replica with the profile readable");
        assert!(without_profile, "no global replica without the profile");
    }
}
