//! UX of the interactive question of `raptor guard exec` (brief guard-exec-ux, UX1 to UX5): an
//! invalid answer explains and asks again, `n`, Ctrl-C and a window that closes unanswered all
//! cancel and leave one `exception-cancelled`, and the prompt says how long there is to answer.
//! Real `raptor` (daemon, client, hook) over the temporary machine of `guard_machine` (NFR-01:
//! temporary repos, remotes, profile and daemon). The exec invocation has no `--yes`, so the
//! question is asked in the pty. Every wait is on an explicit signal or a bounded deadline.
//! Unix only; Windows is pending the cross-platform validation.
#![cfg(unix)]

mod guard_machine;

use std::ffi::OsString;
use std::process::Output;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use guard_machine::{Machine, Running, text};
use serde_json::Value;

fn repo(m: &Machine) -> String {
    m.f.repo.to_string_lossy().into_owned()
}

/// "demo" protected, with `feat-x` rewritten locally (a push needs a force), on `main`.
fn machine(window_ms: u64) -> Machine {
    let m = Machine::with_window(window_ms);
    m.add(&m.f.repo);
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    m.rewrite_feat_x();
    m.git_ok(&m.f.repo, &["switch", "-q", "main"]);
    m
}

/// `raptor guard exec -- git -C <repo> push -f origin feat-x` WITHOUT `--yes`: it asks.
fn exec_asking(m: &Machine) -> Running {
    let repo = repo(m);
    m.developer_spawn(&[
        "guard", "exec", "--", "git", "-C", &repo, "push", "-q", "-f", "origin", "feat-x",
    ])
}

/// The same with `--yes`: it does not ask.
fn exec_yes(m: &Machine) -> Running {
    let repo = repo(m);
    m.developer_spawn(&[
        "guard", "exec", "--yes", "--", "git", "-C", &repo, "push", "-q", "-f", "origin", "feat-x",
    ])
}

/// Waits (bounded) until `guard.status` shows the pending exception: the announcement is in.
fn wait_pending(m: &Machine) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status = m.status_json(&m.f.repo);
        if status["pending"]["action"] == "exception" {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "no pending exception in guard.status (is `raptor guard exec` there?): {status}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn log_entries(m: &Machine) -> Vec<Value> {
    let out = m.developer(&["guard", "log", "--json", &repo(m)]);
    assert!(out.status.success(), "{}", text(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .find_map(|l| l.find('{').map(|i| &l[i..]))
        .unwrap_or_else(|| panic!("no JSON: {stdout}"));
    let log: Value = serde_json::from_str(line.trim()).unwrap();
    log["entries"].as_array().cloned().unwrap_or_default()
}

fn of_kind(entries: &[Value], kind: &str) -> Vec<Value> {
    entries
        .iter()
        .filter(|e| e["kind"] == kind)
        .cloned()
        .collect()
}

fn feat_x_remote(m: &Machine) -> String {
    m.remote_ref("refs/heads/feat-x").expect("feat-x on remote")
}

/// `running.finish()` with a bound: a hung `exec` fails the test (and is killed) instead of
/// hanging the suite.
fn finish_bounded(m: &Machine, running: Running, secs: u64) -> Output {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(running.finish());
    });
    match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(out) => out,
        Err(_) => {
            // The repo path is unique to this machine: it only matches this test's exec.
            let _ = std::process::Command::new("/usr/bin/pkill")
                .args(["-f", &repo(m)])
                .status();
            panic!(
                "`raptor guard exec` was still waiting {secs} s after the pending exception: it never cancelled by itself"
            )
        }
    }
}

/// Starts the asking exec, waits for the announcement, types `input` in one write and waits
/// (bounded) for the end of `exec`.
fn answer(m: &Machine, input: &str) -> Output {
    let mut running = exec_asking(m);
    wait_pending(m);
    running.type_line(input);
    finish_bounded(m, running, 60)
}

/// How many times the terminal showed the `[y/N]` / `[s/N]` prompt.
fn prompts(shown: &str) -> usize {
    shown.matches("/N]").count()
}

/// Asserts the cancellation contract: exec failed, git did not run, one exception-cancelled.
fn assert_cancelled(m: &Machine, out: &Output, before: &str, id: &str) {
    assert!(
        !out.status.success(),
        "{id}: exec should fail when cancelled: {}",
        text(out)
    );
    assert_eq!(feat_x_remote(m), before, "{id}: git was not launched");
    let entries = log_entries(m);
    assert_eq!(
        of_kind(&entries, "exception-cancelled").len(),
        1,
        "{id}: exactly one exception-cancelled entry: {entries:#?}"
    );
    assert!(
        of_kind(&entries, "exception").is_empty(),
        "{id}: {entries:#?}"
    );
}

#[test]
fn invalid_answer_explains_asks_again_and_cancels_after_three() {
    // An invalid answer, then `n`: it explained, asked again and `n` cancelled.
    let m = machine(20_000);
    let before = feat_x_remote(&m);
    let out = answer(&m, "r\nn\n");
    assert_cancelled(&m, &out, &before, "UX1");
    let shown = text(&out);
    assert!(
        shown.to_lowercase().contains("yes"),
        "UX1: an invalid answer should list the valid ones (y/yes/s/si/sí, n/no): {shown}"
    );
    assert!(
        prompts(&shown) >= 2,
        "UX1: after an invalid answer the question should be asked again: {shown}"
    );
    drop(m);

    // Three invalid answers: it cancels and says so.
    let m = machine(20_000);
    let before = feat_x_remote(&m);
    let out = answer(&m, "r\nr\nr\n");
    assert_cancelled(&m, &out, &before, "UX1");
    let shown = text(&out);
    let lower = shown.to_lowercase();
    assert!(
        lower.contains("invalid") || lower.contains("not valid"),
        "UX1: three invalid answers should cancel saying the answers were invalid: {shown}"
    );
    assert!(
        prompts(&shown) >= 3,
        "UX1: it should ask up to three times before cancelling: {shown}"
    );
}

#[test]
fn answer_no_cancels_and_logs() {
    // `n`, and Enter alone (the default is No): the same cancellation and message.
    for (input, what) in [("n\n", "n"), ("\n", "Enter alone")] {
        let m = machine(20_000);
        let before = feat_x_remote(&m);
        let out = answer(&m, input);
        assert_cancelled(&m, &out, &before, "UX2");
        let lower = text(&out).to_lowercase();
        // Only what is printed after the last question counts: the question itself already
        // says that no answer cancels.
        let tail = &lower[lower.rfind("/n]").expect("the prompt was shown")..];
        assert!(
            tail.contains("nothing ran"),
            "UX2: after answering {what} it should say nothing ran: {tail}"
        );
    }
}

#[test]
fn no_answer_at_window_close_cancels_and_logs() {
    let m = machine(3_000);
    let before = feat_x_remote(&m);
    let running = exec_asking(&m);
    wait_pending(&m);
    // Nothing typed: the window closes and exec must end by itself.
    let out = finish_bounded(&m, running, 45);
    assert_cancelled(&m, &out, &before, "UX3");
    let lower = text(&out).to_lowercase();
    let tail = &lower[lower.rfind("/n]").expect("the prompt was shown")..];
    assert!(
        tail.contains("window closed") && tail.contains("cancel"),
        "UX3: exec should say, after the question, it cancelled because the window closed: {tail}"
    );
}

#[test]
fn ctrl_c_at_the_prompt_cancels_and_logs() {
    let m = machine(20_000);
    let before = feat_x_remote(&m);
    let out = answer(&m, "\u{3}");
    assert_cancelled(&m, &out, &before, "UX4");
}

#[test]
fn prompt_states_time_and_default_in_en_and_es() {
    // 7 s of window: the number the prompt must state.
    let m = machine(7_000);
    let out = answer(&m, "n\n");
    let lower = text(&out).to_lowercase();
    assert!(
        lower.contains("7 s"),
        "UX5 (en): the prompt should state the 7 seconds there are to answer: {lower}"
    );
    assert!(
        lower.contains("answer") && lower.contains("cancel"),
        "UX5 (en): the prompt should say that not answering cancels: {lower}"
    );
    drop(m);

    let m = machine(7_000);
    m.extra_env
        .borrow_mut()
        .push(("LANG", OsString::from("es_ES.UTF-8")));
    let out = answer(&m, "n\n");
    let lower = text(&out).to_lowercase();
    assert!(
        lower.contains("7 s"),
        "UX5 (es): el aviso debe decir los 7 segundos que hay para responder: {lower}"
    );
    assert!(
        (lower.contains("respuesta") || lower.contains("respond")) && lower.contains("cancela"),
        "UX5 (es): el aviso debe decir que no responder cancela: {lower}"
    );
    drop(m);

    // With --yes it does not ask and says when it runs.
    let m = machine(2_000);
    let out = exec_yes(&m).finish();
    let shown = text(&out);
    assert!(prompts(&shown) == 0, "UX5: --yes should not ask: {shown}");
    assert!(
        shown.to_lowercase().contains("2 s"),
        "UX5: --yes should say when it runs: {shown}"
    );
}

#[test]
fn answer_yes_in_time_still_runs_git_once() {
    // The success path of the question: `y` before the window closes runs the exception.
    let m = machine(8_000);
    let local = m.git_ok(&m.f.repo, &["rev-parse", "feat-x"]);
    let before = feat_x_remote(&m);
    assert_ne!(local, before);
    let out = answer(&m, "y\n");
    assert!(out.status.success(), "exec should succeed: {}", text(&out));
    assert_eq!(feat_x_remote(&m), local, "the remote has the new commit");
    let entries = log_entries(&m);
    assert_eq!(of_kind(&entries, "exception").len(), 1, "{entries:#?}");
    assert!(
        of_kind(&entries, "exception-cancelled").is_empty(),
        "{entries:#?}"
    );
}
