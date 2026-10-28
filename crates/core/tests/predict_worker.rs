// TS-CKP-001 T006 (PR-B): the predictor's worker process, driven from the daemon side by
// `WorkerMerger` over the real `raptor predict-worker`, on temporary repos (NFR-01). The cuts are
// made deterministic with tightened `WorkerLimits` (S2): a zero deadline or a memory cap of a
// few bytes. No fixed sleeps: idleness is checked with a `now` passed to `close_if_idle`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitraptor_core::predict::merger::{DryMerger, InProcessMerger};
use gitraptor_core::predict::worker::{WorkerConfig, WorkerLimits, WorkerMerger};
use gitraptor_git::dry_merge::{
    ConflictKind, DryMergeError, DryMergeInput, DryMergeLimits, DryMergeOutcome,
};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Fixture, sibling_bin};

/// The daemon's binary, with its `predict-worker` subcommand: `raptor`, built next to this test
/// binary (`target/<profile>/deps/predict_worker-*`).
fn worker_program() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("the test binary lives in target/<profile>/deps");
    sibling_bin(&profile_dir.join("raptor"), "gitraptor-cli", "raptor")
}

fn limits() -> WorkerLimits {
    WorkerLimits {
        // Generous: only the field a test tightens cuts a job.
        deadline: Duration::from_secs(60),
        escalated_deadline: Duration::from_secs(60),
        memory_cap: u64::MAX,
        rss_poll: Duration::from_millis(5),
        idle_exit: Duration::from_secs(60),
    }
}

fn merger(limits: WorkerLimits) -> WorkerMerger {
    WorkerMerger::new(WorkerConfig {
        program: worker_program(),
        limits,
    })
}

fn id(f: &Fixture, rev: &str) -> String {
    f.git(&["rev-parse", rev]).trim().to_owned()
}

/// Commit on `branch` (reset to `from`) the changes `edit` makes; leaves `main` checked out.
fn commit_on(f: &Fixture, branch: &str, from: &str, edit: impl FnOnce(&Fixture)) -> String {
    f.git(&["checkout", "-q", "-B", branch, from]);
    edit(f);
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", branch]);
    let tip = id(f, "HEAD");
    f.git(&["checkout", "-q", "main"]);
    tip
}

fn input(ours: &str, theirs: &str) -> DryMergeInput {
    DryMergeInput {
        ours: ours.to_owned(),
        theirs: theirs.to_owned(),
    }
}

fn common_dir(f: &Fixture) -> PathBuf {
    f.repo.join(".git")
}

fn far() -> Instant {
    Instant::now() + Duration::from_secs(600)
}

fn run(
    m: &dyn DryMerger,
    f: &Fixture,
    pair: &(String, String),
) -> Result<DryMergeOutcome, DryMergeError> {
    m.merge(
        &common_dir(f),
        &input(&pair.0, &pair.1),
        &DryMergeLimits::default(),
        far(),
    )
}

fn lines(prefix: &str, n: u32) -> String {
    (1..=n).map(|i| format!("{prefix}{i}\n")).collect()
}

/// `b.txt` deleted by one side and edited by the other: a `ModifyDelete` conflict.
fn modify_delete() -> (Fixture, (String, String)) {
    let f = Fixture::with_commit(&git_from_path());
    let ours = commit_on(&f, "ours", "main", |f| {
        std::fs::remove_file(f.repo.join("b.txt")).unwrap();
    });
    let theirs = commit_on(&f, "theirs", "main", |f| f.write("b.txt", "beta changed\n"));
    (f, (ours, theirs))
}

/// `r.txt` renamed to a different name by each side: a `Rename` conflict.
fn rename() -> (Fixture, (String, String)) {
    let f = Fixture::with_commit(&git_from_path());
    f.write("r.txt", &lines("row", 30));
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", "r"]);
    let ours = commit_on(&f, "ours", "main", |f| {
        f.git(&["mv", "r.txt", "r-ours.txt"]);
    });
    let theirs = commit_on(&f, "theirs", "main", |f| {
        f.git(&["mv", "r.txt", "r-theirs.txt"]);
    });
    (f, (ours, theirs))
}

/// Two edits of line 5 of `a.txt`: a `Content` conflict that needs no escalation.
fn content_conflict() -> (Fixture, (String, String)) {
    let f = Fixture::with_commit(&git_from_path());
    f.write("a.txt", &lines("l", 20));
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", "a"]);
    let edit = |side: &'static str| {
        move |f: &Fixture| {
            f.write(
                "a.txt",
                &lines("l", 20).replace("l5\n", &format!("{side}\n")),
            );
        }
    };
    let ours = commit_on(&f, "ours", "main", edit("ours-5"));
    let theirs = commit_on(&f, "theirs", "main", edit("theirs-5"));
    (f, (ours, theirs))
}

/// Two branches that touch different files: a clean merge.
fn clean() -> (Fixture, (String, String)) {
    let f = Fixture::with_commit(&git_from_path());
    let ours = commit_on(&f, "ours", "main", |f| f.write("o.txt", "o\n"));
    let theirs = commit_on(&f, "theirs", "main", |f| f.write("t.txt", "t\n"));
    (f, (ours, theirs))
}

#[test]
fn a_pair_over_the_deadline_kills_and_relaunches_the_worker() {
    let (f, pair) = content_conflict();
    let m = merger(WorkerLimits {
        deadline: Duration::ZERO,
        escalated_deadline: Duration::ZERO,
        ..limits()
    });

    let first = run(&m, &f, &pair);
    assert_eq!(
        first,
        Err(DryMergeError::LimitExceeded),
        "a job past its deadline must come back as LimitExceeded"
    );
    assert_eq!(m.launches(), 1, "one worker was launched for the first job");

    let second = run(&m, &f, &pair);
    assert_eq!(
        second,
        Err(DryMergeError::LimitExceeded),
        "the next job runs (and is cut) in a relaunched worker"
    );
    assert_eq!(
        m.launches(),
        2,
        "the worker killed by the deadline must be relaunched for the next job"
    );
}

#[test]
fn a_worker_over_the_memory_cap_is_killed() {
    let (f, pair) = content_conflict();
    let m = merger(WorkerLimits {
        memory_cap: 1,
        rss_poll: Duration::from_millis(1),
        ..limits()
    });

    let got = run(&m, &f, &pair);
    assert_eq!(
        got,
        Err(DryMergeError::LimitExceeded),
        "a worker whose resident memory passes the cap must be killed: LimitExceeded"
    );
    assert_eq!(m.launches(), 1);
    assert_eq!(
        m.worker_pid(),
        None,
        "the worker killed for its memory must not stay alive"
    );
}

#[test]
fn a_cut_escalation_publishes_the_first_result() {
    for (name, (f, pair), kind) in [
        ("modify/delete", modify_delete(), ConflictKind::ModifyDelete),
        ("rename", rename(), ConflictKind::Rename),
    ] {
        // The second job (rewrites limit raised) is cut: the first job's result is published.
        let cut = merger(WorkerLimits {
            escalated_deadline: Duration::ZERO,
            ..limits()
        });
        let got = run(&cut, &f, &pair)
            .unwrap_or_else(|e| panic!("{name}: a cut escalation must not be an error, got {e:?}"));
        assert!(
            got.files.iter().any(|c| c.kind == kind),
            "{name}: the first result's conflict must be published: {:?}",
            got.files
        );
        assert!(
            !got.rewrites_escalated,
            "{name}: a cut escalation publishes the first result, not escalated"
        );

        // The second job ends: its result is published, marked escalated.
        let whole = merger(limits());
        let got = run(&whole, &f, &pair)
            .unwrap_or_else(|e| panic!("{name}: the escalated job must end, got {e:?}"));
        assert!(
            got.rewrites_escalated,
            "{name}: a second job that ends must publish rewrites_escalated"
        );
        assert!(!got.files.is_empty(), "{name}: the conflict stays");
    }

    // A pair with no modify/delete or rename conflict is never escalated, whatever the deadline.
    let (f, pair) = content_conflict();
    let m = merger(WorkerLimits {
        escalated_deadline: Duration::ZERO,
        ..limits()
    });
    let got = run(&m, &f, &pair).expect("a content conflict needs no second job");
    assert!(!got.rewrites_escalated);
    assert_eq!(got.files.len(), 1);
}

mod lifecycle {
    use super::*;

    #[test]
    fn the_worker_launches_with_the_first_job_and_not_before() {
        let (f, pair) = clean();
        let m = merger(limits());
        assert_eq!(m.launches(), 0, "no worker before the first job");
        assert_eq!(m.worker_pid(), None);

        run(&m, &f, &pair).expect("a clean merge");
        assert_eq!(m.launches(), 1);
        assert!(
            m.worker_pid().is_some(),
            "the worker stays alive after a job"
        );

        run(&m, &f, &pair).expect("a second job");
        assert_eq!(
            m.launches(),
            1,
            "the same long-lived worker serves both jobs"
        );
    }

    #[test]
    fn the_worker_gives_the_same_result_as_the_in_process_merger() {
        let m = merger(limits());
        for (f, pair) in [clean(), content_conflict()] {
            let in_process = run(&InProcessMerger, &f, &pair).expect("in-process merge");
            let from_worker = run(&m, &f, &pair).expect("worker merge");
            assert_eq!(
                from_worker, in_process,
                "the worker must answer what InProcessMerger answers"
            );
        }
    }

    #[test]
    fn the_worker_closes_after_idle_exit_without_work() {
        let (f, pair) = clean();
        let m = merger(limits());
        assert!(
            !m.close_if_idle(Instant::now()),
            "with no worker there is nothing to close"
        );
        run(&m, &f, &pair).expect("a clean merge");
        assert!(m.worker_pid().is_some());

        assert!(
            !m.close_if_idle(Instant::now()),
            "a worker that just worked is not idle"
        );
        assert!(m.worker_pid().is_some());

        let later = Instant::now() + limits().idle_exit + Duration::from_secs(1);
        assert!(
            m.close_if_idle(later),
            "idle_exit without work closes the worker"
        );
        assert_eq!(m.worker_pid(), None);

        run(&m, &f, &pair).expect("a merge after the close");
        assert_eq!(m.launches(), 2, "a later merge relaunches the worker");
    }

    #[test]
    fn close_closes_the_worker_and_a_later_merge_relaunches_it() {
        let (f, pair) = clean();
        let m = merger(limits());
        run(&m, &f, &pair).expect("a clean merge");
        assert!(m.worker_pid().is_some());

        m.close();
        assert_eq!(m.worker_pid(), None, "close() must end the worker");

        run(&m, &f, &pair).expect("a merge after close()");
        assert_eq!(m.launches(), 2);
        assert!(m.worker_pid().is_some());
        m.close();
    }
}

mod wiring {
    use gitraptor_core::predict::{PredictConfig, PredictMerger};

    use super::*;

    #[test]
    fn for_daemon_is_the_worker_of_this_binary_with_the_default_limits() {
        let PredictConfig { merger } = PredictConfig::for_daemon();
        let PredictMerger::Worker(config) = merger else {
            panic!("for_daemon must give PredictMerger::Worker, got {merger:?}");
        };
        assert!(
            config.program.is_absolute(),
            "the worker program is absolute"
        );
        assert_eq!(
            config.program,
            std::env::current_exe().unwrap(),
            "the worker is the daemon's own binary, resolved once by current_exe"
        );
        assert_eq!(config.limits, WorkerLimits::default());
    }
}

mod version {
    use super::*;

    #[test]
    fn a_version_mismatch_relaunches_once_and_then_the_merge_is_unavailable() {
        let (f, pair) = clean();
        let m = WorkerMerger::with_expected_version(
            WorkerConfig {
                program: worker_program(),
                limits: limits(),
            },
            "0.0.0-mismatch",
        );
        let got = run(&m, &f, &pair);
        assert!(
            matches!(got, Err(DryMergeError::Unavailable(_))),
            "a worker of another version must end in Unavailable, got {got:?}"
        );
        assert_eq!(
            m.launches(),
            2,
            "the worker of the wrong version is relaunched once, and no more"
        );
    }
}

/// The worker's environment and working directory, read from the live child. The test process
/// cannot set variables itself (the workspace forbids `unsafe`), so the check runs in a child of
/// this test binary that does start with them: `inner_probe`, run only by `the_worker_environment…`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod env {
    use std::process::Command;

    use super::*;

    const PROGRAM_VAR: &str = "RAPTOR_TEST_WORKER_PROGRAM";
    const CANARY: &str = "RAPTOR_TEST_CANARY";

    #[test]
    fn the_worker_environment_is_built_from_scratch_and_its_cwd_is_outside_any_repo() {
        let program = worker_program();
        let exe = std::env::current_exe().unwrap();
        let out = Command::new(exe)
            .args(["--ignored", "--exact", "env::inner_probe", "--nocapture"])
            .env(PROGRAM_VAR, &program)
            // What the daemon may carry and the worker must not inherit.
            .env("GIT_DIR", "/nonexistent-git-dir")
            .env("GIT_CONFIG_GLOBAL", "/nonexistent-gitconfig")
            .env("GIT_EXEC_PATH", "/nonexistent-exec-path")
            .env("DYLD_LIBRARY_PATH", "/nonexistent-dyld")
            .env("LD_LIBRARY_PATH", "/nonexistent-ld")
            .env(CANARY, "1")
            .env("PATH", "/usr/bin:/bin:relative/bin:.:")
            .output()
            .expect("run the probe");
        assert!(
            out.status.success(),
            "the worker's environment broke a rule:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Environment variables of the live process `pid`.
    fn environment_of(pid: u32) -> Vec<(String, String)> {
        #[cfg(target_os = "linux")]
        {
            let raw = std::fs::read(format!("/proc/{pid}/environ")).expect("read environ");
            raw.split(|b| *b == 0)
                .filter(|v| !v.is_empty())
                .map(|v| {
                    let text = String::from_utf8_lossy(v).into_owned();
                    match text.split_once('=') {
                        Some((k, v)) => (k.to_owned(), v.to_owned()),
                        None => (text, String::new()),
                    }
                })
                .collect()
        }
        #[cfg(target_os = "macos")]
        {
            // `ps -E` appends the environment to the command, separated by spaces.
            let out = Command::new("/bin/ps")
                .args(["-wwE", "-o", "command=", "-p", &pid.to_string()])
                .output()
                .expect("run ps");
            assert!(out.status.success(), "ps could not read the worker {pid}");
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let mut vars: Vec<(String, String)> = Vec::new();
            for token in text.split_whitespace() {
                let named = token.split_once('=').filter(|(k, _)| {
                    !k.is_empty()
                        && k.chars()
                            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                });
                match named {
                    Some((k, v)) => vars.push((k.to_owned(), v.to_owned())),
                    // A value with a space: the rest belongs to the last variable.
                    None => {
                        if let Some(last) = vars.last_mut() {
                            last.1.push(' ');
                            last.1.push_str(token);
                        }
                    }
                }
            }
            vars
        }
    }

    /// Working directory of the live process `pid`.
    fn cwd_of(pid: u32) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            std::fs::read_link(format!("/proc/{pid}/cwd")).expect("read cwd")
        }
        #[cfg(target_os = "macos")]
        {
            let out = Command::new("/usr/sbin/lsof")
                .args(["-a", "-d", "cwd", "-p", &pid.to_string(), "-Fn"])
                .output()
                .expect("run lsof");
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let line = text
                .lines()
                .find_map(|l| l.strip_prefix('n'))
                .unwrap_or_else(|| panic!("lsof gave no cwd for {pid}: {text}"));
            PathBuf::from(line)
        }
    }

    #[test]
    #[ignore = "run by the_worker_environment_is_built_from_scratch_and_its_cwd_is_outside_any_repo"]
    fn inner_probe() {
        let program = PathBuf::from(std::env::var_os(PROGRAM_VAR).expect("the probe's program"));
        let (f, pair) = clean();
        let m = WorkerMerger::new(WorkerConfig {
            program,
            limits: limits(),
        });
        run(&m, &f, &pair).expect("a clean merge launches the worker");
        let pid = m.worker_pid().expect("a live worker");

        let vars = environment_of(pid);
        let names: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        for (k, _) in &vars {
            assert!(
                !k.starts_with("GIT_") && !k.starts_with("DYLD_") && !k.starts_with("LD_"),
                "the worker must not inherit {k}: {names:?}"
            );
        }
        assert!(
            !names.contains(&CANARY),
            "the environment is built from scratch: {CANARY} leaked into {names:?}"
        );
        assert!(
            names.contains(&"HOME"),
            "HOME is one of the few variables: {names:?}"
        );
        let path = vars
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        for entry in path.split(':').filter(|_| !path.is_empty()) {
            assert!(
                Path::new(entry).is_absolute(),
                "the worker's PATH has the relative entry {entry:?}: {path}"
            );
        }

        let cwd = cwd_of(pid);
        assert!(cwd.is_absolute(), "cwd {}", cwd.display());
        let inside_repo = cwd.ancestors().any(|dir| {
            dir.join(".git").exists() || dir.join("HEAD").is_file() && dir.join("objects").is_dir()
        });
        assert!(
            !inside_repo,
            "the worker's cwd {} is inside a repo",
            cwd.display()
        );
        assert!(
            !cwd.starts_with(&f.root),
            "the worker's cwd {} is in the repo's fixture",
            cwd.display()
        );
        m.close();
    }
}
