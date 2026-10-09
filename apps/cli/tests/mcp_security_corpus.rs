//! MCP security corpus (Q-MCP-18): every attack case must be rejected, and the report prints the
//! share rejected.
//!
//! Debug builds only: `GITRAPTOR_PROFILE_DIR` does not exist in release, so there the harness
//! would touch the real profile.
#![cfg(debug_assertions)]

mod mcp_corpus;

use gitraptor_testkit::mcp_corpus::{
    Case, Expect, Failure, Outcome, Platform, Report, Row, Tier, Verdict, judge,
};
use mcp_corpus::{cases_dir, limits, load_corpus, run_case, run_corpus, write_report};
use serde_json::{Value, json};

#[cfg(unix)]
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";

/// Entry point of the simulated agent: when this test binary runs as `raptor-fake-agent` with
/// `RAPTOR_FAKE_AGENT_ARGV`, it runs that command as its child and exits with its status. As a
/// normal test it does nothing.
#[cfg(unix)]
#[test]
fn fake_agent_entry() {
    let Some(argv) = std::env::var_os(FAKE_AGENT_ARGV) else {
        return;
    };
    let argv: Vec<String> = serde_json::from_str(argv.to_str().unwrap()).unwrap();
    let status = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .env_remove(FAKE_AGENT_ARGV)
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

fn corpus() -> Vec<Case> {
    match load_corpus() {
        Ok(cases) => cases,
        Err(errors) => panic!("the corpus does not load:\n{}", errors.join("\n")),
    }
}

fn case_named(id: &str) -> Case {
    let found = corpus().into_iter().find(|case| case.id == id);
    match found {
        Some(case) => case,
        None => panic!("no case named {id} in {}", cases_dir().display()),
    }
}

fn failures(verdict: Verdict) -> Vec<Failure> {
    match verdict {
        Verdict::Failed(failures) => failures,
        Verdict::Rejected | Verdict::KnownGap(_) => Vec::new(),
    }
}

#[test]
fn every_case_file_is_valid_unique_and_named_after_its_id() {
    assert!(cases_dir().is_dir(), "{}", cases_dir().display());
    let cases = corpus();
    assert!(cases.len() >= 40, "only {} cases", cases.len());
    for tier in [Tier::Server, Tier::Engine] {
        assert!(cases.iter().any(|c| c.tier == tier), "no {tier:?} case");
    }
    let has = |pick: fn(&Expect) -> bool| cases.iter().any(|c| pick(&c.expect));
    assert!(has(|e| matches!(e, Expect::Refusal { .. })));
    assert!(has(|e| matches!(e, Expect::ProtocolError { .. })));
    assert!(has(|e| matches!(e, Expect::InvalidRequest)));
    assert!(has(|e| matches!(e, Expect::Ignored)));
}

#[test]
fn the_corpus_is_fully_rejected() {
    let cases = corpus();
    let report = run_corpus(&cases);
    let path = write_report(&report);
    println!("{}", report.summary_line());
    println!("report: {}", path.display());
    assert_eq!(report.gate(), Ok(()), "{}", report.markdown());
    assert!(report.executed_in(Tier::Server) >= 1);
    if matches!(Platform::current(), Some(Platform::Macos | Platform::Linux)) {
        assert!(report.executed_in(Tier::Engine) >= 1);
    }
}

#[test]
fn an_injected_pass_of_a_traversal_case_fails_the_suite() {
    let case = case_named("status-cursor-traversal");
    let mut run = run_case(&case);
    assert_eq!(
        judge(&case, &run.observation, &run.secrets, &limits()),
        Verdict::Rejected
    );

    let answer = run.observation.answers.first_mut();
    assert!(answer.is_some(), "the real session answered nothing");
    if let Some(answer) = answer {
        answer.message = json!({"jsonrpc": "2.0", "id": 10, "result": {
            "content": [{"type": "text", "text": "{}"}],
            "structuredContent": {}
        }});
    }
    let verdict = judge(&case, &run.observation, &run.secrets, &limits());
    assert!(
        failures(verdict.clone()).contains(&Failure::NotRejected),
        "{verdict:?}"
    );

    let report = Report {
        os: "test".into(),
        rows: vec![Row {
            id: case.id.clone(),
            tier: case.tier,
            outcome: match verdict {
                Verdict::Failed(failures) => Outcome::Failed(failures),
                Verdict::Rejected => Outcome::Rejected,
                Verdict::KnownGap(reference) => Outcome::KnownGap(reference),
            },
        }],
    };
    assert!(report.gate().is_err());
    assert!(
        report.summary_line().contains("0/1 rejected (0.0 %)"),
        "{}",
        report.summary_line()
    );
}

fn has_field_not_allowed(failures: &[Failure]) -> bool {
    failures
        .iter()
        .any(|f| matches!(f, Failure::FieldNotAllowed { .. }))
}

#[test]
fn an_undeclared_field_in_a_real_refusal_breaks_the_allowlist() {
    let case = case_named("snapshot-label-too-long");
    let mut run = run_case(&case);
    assert_eq!(
        judge(&case, &run.observation, &run.secrets, &limits()),
        Verdict::Rejected
    );
    let text = run
        .observation
        .answers
        .first_mut()
        .and_then(|a| a.message.pointer_mut("/result/content/0/text"));
    assert!(text.is_some(), "the refusal has no text block");
    if let Some(text) = text {
        let mut body: Value = serde_json::from_str(text.as_str().unwrap_or("{}")).unwrap();
        body["repo_path"] = json!("/home/someone/repo");
        *text = Value::String(body.to_string());
    }
    let found = failures(judge(&case, &run.observation, &run.secrets, &limits()));
    assert!(has_field_not_allowed(&found), "{found:?}");

    let case = case_named("status-param-repo");
    let mut run = run_case(&case);
    let error = run
        .observation
        .answers
        .first_mut()
        .and_then(|a| a.message.pointer_mut("/error"));
    assert!(error.is_some(), "the error answer has no error object");
    if let Some(error) = error {
        error["data"]["secret"] = json!("x");
    }
    let found = failures(judge(&case, &run.observation, &run.secrets, &limits()));
    assert!(has_field_not_allowed(&found), "{found:?}");
}
