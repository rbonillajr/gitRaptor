//! TS-CKP-001 T006 (CKPB-WORKER-CLI): `raptor predict-worker` is the hidden subcommand the
//! daemon launches as the predictor's worker. It is not in `--help`, it ends with 0 on EOF of its
//! stdin and it opens nothing of the profile (ADR-GRP-005: the daemon is its only writer).

use std::process::{Command, Output, Stdio};

fn raptor(profile: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_raptor"))
        .args(args)
        .env("GITRAPTOR_PROFILE_DIR", profile)
        .stdin(Stdio::null())
        .output()
        .expect("run raptor")
}

fn describe(out: &Output) -> String {
    format!(
        "status {:?}\nstdout: {}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn predict_worker_exits_zero_on_eof_of_stdin() {
    let profile = tempfile::tempdir().unwrap();
    let out = raptor(profile.path(), &["predict-worker"]);
    assert!(
        out.status.success(),
        "`raptor predict-worker` must end with 0 on EOF of stdin\n{}",
        describe(&out)
    );
}

#[test]
fn predict_worker_is_hidden_from_help_but_exists() {
    let profile = tempfile::tempdir().unwrap();
    let own = raptor(profile.path(), &["predict-worker", "--help"]);
    assert!(
        own.status.success(),
        "the subcommand must exist: `raptor predict-worker --help`\n{}",
        describe(&own)
    );
    let help = raptor(profile.path(), &["--help"]);
    assert!(help.status.success(), "{}", describe(&help));
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(
        !text.contains("predict-worker"),
        "predict-worker must be hidden from `raptor --help`:\n{text}"
    );
}

#[test]
fn predict_worker_leaves_the_profile_empty() {
    let profile = tempfile::tempdir().unwrap();
    let out = raptor(profile.path(), &["predict-worker"]);
    assert!(
        out.status.success(),
        "the worker must run to EOF before its profile is checked\n{}",
        describe(&out)
    );
    let left: Vec<_> = std::fs::read_dir(profile.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(
        left.is_empty(),
        "the worker must create nothing in the profile, found {left:?}"
    );
}
