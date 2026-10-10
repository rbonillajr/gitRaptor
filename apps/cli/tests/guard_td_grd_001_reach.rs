//! The forbidden paths reach every pushed ref, end to end: the real `raptor` as daemon and hook,
//! the real `raptor-hook` dispatcher (template 3) and plain Git, over a temporary machine and a
//! bare remote (NFR-01). The agent is `raptor-fake-agent`, a copy of this test binary the debug
//! daemon knows as Claude Code, so the actor comes from the ancestry. No fixed waits.
//!
//! Unix only; Windows has no channel transport yet (Pendiente: etapa de validación
//! multiplataforma).
#![cfg(unix)]

mod guard_machine;
mod td_grd_001_machine;

use td_grd_001_machine::{AGENT_PATHS, EVERYONE_PATHS, FORBIDDEN, NO_PATHS, Td, text};

#[test]
fn fake_agent_entry() {
    td_grd_001_machine::fake_agent_entry();
}

/// An agent pushing to `target` a commit that touches a forbidden path is denied and the ref
/// never reaches the remote; a clean commit to the same ref goes in.
fn forbidden_path_to(target: &str) {
    let m = Td::new(Some(AGENT_PATHS), true);
    let bad = m.commit_on("bad", FORBIDDEN);
    let clean = m.commit_on("clean", "ok.txt");

    m.git_ok(&m.repo(), &["switch", "-q", "bad"]);
    let out = m.agent_push(&format!("HEAD:{target}"));
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains(FORBIDDEN), "{}", text(&out));
    assert_eq!(m.remote_ref(target), None, "the ref reached the remote");

    m.git_ok(&m.repo(), &["switch", "-q", "clean"]);
    let out = m.agent_push(&format!("HEAD:{target}"));
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(m.remote_ref(target), Some(clean));
    assert_ne!(m.remote_ref(target), Some(bad));
}

// Escenario de la ficha: un tag.
#[test]
fn an_agent_cannot_push_a_forbidden_path_to_a_tag() {
    forbidden_path_to("refs/tags/x");
}

#[test]
fn an_agent_cannot_push_a_forbidden_path_to_notes() {
    forbidden_path_to("refs/notes/x");
}

// `refs/remotes/*` follows the same code path as `refs/bisect/*` and `refs/rewritten/*`.
#[test]
fn an_agent_cannot_push_a_forbidden_path_to_another_ungoverned_ref() {
    forbidden_path_to("refs/remotes/mirror/smuggled");
}

// Caso A: without `raptor` the dispatcher cannot ask, and a push of only a tag goes ahead with
// the warning; a branch still fails closed.
#[test]
fn without_raptor_only_tags_go_ahead_with_a_warning() {
    let m = Td::new(Some(AGENT_PATHS), true);
    let _ = m.commit_on("work", "ok.txt");
    m.set_constant("raptor", "/nonexistent/raptor");
    m.git_ok(&m.repo(), &["switch", "-q", "work"]);

    let out = m.human_push("HEAD:refs/tags/x");
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("protection inactive"), "{}", text(&out));
    assert!(m.remote_ref("refs/tags/x").is_some());

    let out = m.human_push("HEAD:refs/heads/work");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("was not found"), "{}", text(&out));
    assert_eq!(m.remote_ref("refs/heads/work"), None);
}

// Caso B, sin regla: a person without path rules is never blocked, daemon or not, and the
// output shows the push was evaluated (the degraded note) and not skipped.
#[test]
fn with_the_daemon_down_a_tag_push_without_path_rules_goes_ahead() {
    let m = Td::new(Some(NO_PATHS), true);
    let _ = m.commit_on("bad", FORBIDDEN);
    m.git_ok(&m.repo(), &["switch", "-q", "bad"]);
    m.stop();

    let out = m.human_push("HEAD:refs/tags/x");
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("service unreachable"), "{}", text(&out));
    assert!(m.remote_ref("refs/tags/x").is_some());
}

// Caso B, regla para agentes: the actor cannot be told apart without the daemon, so the path is
// denied and the message says how to go on.
#[test]
fn with_the_daemon_down_a_path_forbidden_to_agents_on_a_tag_is_denied_with_how_to_continue() {
    let m = Td::new(Some(AGENT_PATHS), true);
    let _ = m.commit_on("bad", FORBIDDEN);
    let clean = m.commit_on("clean", "ok.txt");
    m.stop();

    m.git_ok(&m.repo(), &["switch", "-q", "bad"]);
    let out = m.agent_push("HEAD:refs/tags/x");
    assert!(!out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains(FORBIDDEN), "{shown}");
    assert!(shown.contains("To go on: start GitRaptor"), "{shown}");
    assert_eq!(m.remote_ref("refs/tags/x"), None);

    m.git_ok(&m.repo(), &["switch", "-q", "clean"]);
    let out = m.agent_push("HEAD:refs/tags/x");
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(m.remote_ref("refs/tags/x"), Some(clean));
}

// O-1: in degraded mode the rules for everyone of the floor reach the refs Guardrails does not
// govern.
#[test]
fn in_degraded_mode_an_everyone_rule_denies_a_forbidden_path_to_a_tag() {
    // The commits come first: once protected, the rule for everyone refuses the person's own
    // commit of the forbidden path to a branch.
    let m = Td::new(Some(EVERYONE_PATHS), false);
    let _ = m.commit_on("bad", FORBIDDEN);
    let clean = m.commit_on("clean", "ok.txt");
    let out = m.protect(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    m.stop();

    m.git_ok(&m.repo(), &["switch", "-q", "bad"]);
    let out = m.human_push("HEAD:refs/tags/x");
    assert!(!out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains(FORBIDDEN), "{shown}");
    assert!(shown.contains("service unreachable"), "{shown}");
    assert_eq!(m.remote_ref("refs/tags/x"), None);

    m.git_ok(&m.repo(), &["switch", "-q", "clean"]);
    let out = m.human_push("HEAD:refs/tags/x");
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(m.remote_ref("refs/tags/x"), Some(clean));
}

/// Evidence for the PR, not a gate (a time assert never runs in a debug test): the cost of a
/// push of tags with template 2 and with template 3, with and without a path rule. Run with
/// `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo test --release -p gitraptor-cli --test
/// guard_td_grd_001_reach latency_report_push_to_a_tag -- --ignored --nocapture`.
#[test]
#[ignore = "measurement: release build, idle machine"]
fn latency_report_push_to_a_tag() {
    use std::time::Instant;
    const RUNS: usize = 20;
    for (rules, settings) in [("no rule", NO_PATHS), ("path rule", AGENT_PATHS)] {
        for template in [2, 3] {
            for tags in [1usize, 5] {
                let m = Td::new(Some(settings), true);
                if template == 2 {
                    m.downgrade_to(2);
                    let out = m.raptor(&["daemon", "status"]);
                    assert!(out.status.success(), "{}", text(&out));
                }
                let _ = m.commit_on("work", "ok.txt");
                m.git_ok(&m.repo(), &["switch", "-q", "work"]);
                let mut millis = Vec::new();
                for run in 0..RUNS {
                    let names: Vec<String> = (0..tags)
                        .map(|t| format!("HEAD:refs/tags/t{run}-{t}"))
                        .collect();
                    let mut args = vec!["push", "origin"];
                    args.extend(names.iter().map(String::as_str));
                    let start = Instant::now();
                    let out = m.git(&m.repo(), &args);
                    millis.push(start.elapsed().as_secs_f64() * 1000.0);
                    assert!(out.status.success(), "{}", text(&out));
                }
                millis.sort_by(f64::total_cmp);
                eprintln!(
                    "template {template}, {tags} tag(s), {rules}: p50 {:.0} ms, p95 {:.0} ms",
                    millis[RUNS / 2],
                    millis[RUNS * 95 / 100]
                );
            }
        }
    }
}
