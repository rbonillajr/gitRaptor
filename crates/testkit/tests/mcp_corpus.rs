//! Contract tests of the MCP corpus logic: case model, judge, scanners and report. Pure: they
//! build their JSON and observations inline and run on every OS.

use std::collections::BTreeMap;

use gitraptor_testkit::mcp_corpus::case::{
    Anchor, Link, Location, Parent, RepoState, Send, Session, Setup,
};
use gitraptor_testkit::mcp_corpus::scan::{hidden_characters, is_hidden, stderr_fixed_codes};
use gitraptor_testkit::mcp_corpus::schema::conforms;
use gitraptor_testkit::mcp_corpus::{
    Answer, Case, CaseError, Expect, Failure, Limits, Observation, Outcome, Platform, Report, Row,
    Secrets, Stream, Tier, Verdict, judge, parse,
};
use serde_json::{Value, json};

const PART_BYTES: usize = 24 * 1024;
const REFUSAL_BYTES: usize = 240;

fn limits() -> Limits {
    Limits {
        part_bytes: PART_BYTES,
        refusal_bytes: REFUSAL_BYTES,
        name_chars: 100,
    }
}

fn server_json() -> Value {
    json!({
        "id": "status-cursor-traversal",
        "title": "A cursor that is a traversal path is malformed and reaches no engine",
        "threats": ["MCP05", "SEC-MCP-05", "BR-MCP-VAL-005"],
        "tier": "server",
        "platforms": ["macos", "linux", "windows"],
        "send": [{"call": "status", "arguments": {"cursor": "../../../../etc/passwd"}}],
        "expect": {"protocol_error": {"code": -32602, "message": "invalid-params", "field": "cursor"}}
    })
}

fn engine_json() -> Value {
    json!({
        "id": "scope-symlink-into-not-enabled",
        "title": "A cwd reached through a symlink resolves to the real, not enabled repo",
        "threats": ["MCP02", "SEC-MCP-02"],
        "tier": "engine",
        "platforms": ["macos", "linux"],
        "pending": "XP-42",
        "setup": {"repo": "enabled", "other_repo": "observed",
                  "symlinks": [{"at": "repo/into-other", "to": "other_repo"}]},
        "session": {"cwd": "repo/into-other"},
        "send": [{"call": "status"}],
        "expect": {"refusal": {"code": "repo-not-enabled"}},
        "forbidden": ["{other_repo}", "{repo_id:other_repo}"]
    })
}

fn parse_value(value: &Value) -> Result<Case, CaseError> {
    parse(&value.to_string())
}

fn repo_location(rel: &[&str]) -> Location {
    Location {
        anchor: Anchor::Repo,
        rel: rel.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// A case built by hand, so the judge tests do not depend on the parser.
fn case_with(send: Vec<Send>, expect: Expect) -> Case {
    Case {
        id: "case".into(),
        title: "A case".into(),
        threats: vec!["MCP05".into()],
        tier: Tier::Server,
        platforms: Platform::ALL.to_vec(),
        pending: None,
        known_gap: None,
        setup: Setup::default(),
        session: Session {
            cwd: repo_location(&[]),
            env: BTreeMap::new(),
            parent: Parent::Unattributed,
        },
        send,
        expect,
        forbidden: Vec::new(),
    }
}

fn status_call() -> Send {
    Send::Call {
        tool: "status".into(),
        arguments: None,
        repeat: 1,
    }
}

fn cursor_case() -> Case {
    case_with(
        vec![status_call()],
        Expect::ProtocolError {
            code: -32602,
            message: Some("invalid-params".into()),
            field: Some("cursor".into()),
        },
    )
}

fn refusal_case() -> Case {
    case_with(
        vec![status_call()],
        Expect::Refusal {
            code: "invalid-text".into(),
            params: vec!["field".into(), "max_chars".into()],
        },
    )
}

fn status_tool() -> Value {
    json!({
        "name": "status",
        "outputSchema": {
            "type": "object",
            "properties": {"repo": {"type": "string"}},
            "required": ["repo"],
            "additionalProperties": false
        }
    })
}

fn protocol_error(code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({"jsonrpc": "2.0", "id": 10, "error": error})
}

fn cursor_error() -> Value {
    protocol_error(
        -32602,
        "invalid-params",
        Some(json!({"field": {"untrusted": "cursor"}})),
    )
}

fn success(structured: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 10, "result": {
        "content": [{"type": "text", "text": structured.to_string()}],
        "structuredContent": structured
    }})
}

fn refusal_text(text: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 10, "result": {
        "isError": true,
        "content": [{"type": "text", "text": text.to_string()}]
    }})
}

fn invalid_text_refusal() -> Value {
    refusal_text(&json!({
        "code": "invalid-text",
        "message": "The label is not valid text.",
        "action": "Use plain text.",
        "params": {"field": "label", "max_chars": 64}
    }))
}

/// A session that ran to the end, with these answers to the `status` call.
fn observation(messages: Vec<Value>) -> Observation {
    let mut stdout = vec![r#"{"jsonrpc":"2.0","id":1,"result":{}}"#.to_owned()];
    stdout.extend(messages.iter().map(Value::to_string));
    Observation {
        tools: vec![status_tool()],
        answers: messages
            .into_iter()
            .map(|message| Answer {
                tool: Some("status".into()),
                message,
            })
            .collect(),
        stdout,
        stderr: String::new(),
        sentinel_answered: true,
        ..Observation::default()
    }
}

fn failures(verdict: Verdict) -> Vec<Failure> {
    match verdict {
        Verdict::Failed(failures) => failures,
        Verdict::Rejected | Verdict::KnownGap(_) => Vec::new(),
    }
}

fn judged(case: &Case, observation: &Observation) -> Vec<Failure> {
    failures(judge(case, observation, &Secrets::default(), &limits()))
}

fn field_not_allowed(failures: &[Failure]) -> bool {
    failures
        .iter()
        .any(|f| matches!(f, Failure::FieldNotAllowed { .. }))
}

fn row(id: &str, outcome: Outcome) -> Row {
    Row {
        id: id.into(),
        tier: Tier::Server,
        outcome,
    }
}

#[test]
fn a_case_file_parses_into_its_model() {
    let server = parse_value(&server_json());
    let Ok(server) = server else {
        panic!("the server example must parse: {server:?}");
    };
    assert_eq!(server.id, "status-cursor-traversal");
    assert_eq!(server.tier, Tier::Server);
    assert_eq!(server.platforms, Platform::ALL.to_vec());
    assert_eq!(server.pending, None);
    assert_eq!(server.known_gap, None);
    assert_eq!(server.setup, Setup::default());
    assert_eq!(
        server.expect,
        Expect::ProtocolError {
            code: -32602,
            message: Some("invalid-params".into()),
            field: Some("cursor".into()),
        }
    );
    assert_eq!(server.expected_answers(), 1);
    assert!(server.runs_on(Platform::Windows));

    let engine = parse_value(&engine_json());
    let Ok(engine) = engine else {
        panic!("the engine example must parse: {engine:?}");
    };
    assert_eq!(engine.tier, Tier::Engine);
    assert_eq!(engine.platforms, vec![Platform::Macos, Platform::Linux]);
    assert_eq!(engine.pending.as_deref(), Some("XP-42"));
    assert_eq!(engine.setup.repo, RepoState::Enabled);
    assert_eq!(engine.setup.other_repo, RepoState::Observed);
    assert_eq!(
        engine.setup.symlinks,
        vec![Link {
            at: repo_location(&["into-other"]),
            to: Location {
                anchor: Anchor::OtherRepo,
                rel: Vec::new()
            },
        }]
    );
    assert_eq!(engine.session.cwd, repo_location(&["into-other"]));
    assert_eq!(
        engine.expect,
        Expect::Refusal {
            code: "repo-not-enabled".into(),
            params: Vec::new(),
        }
    );
    assert_eq!(engine.forbidden.len(), 2);
    assert_eq!(engine.expected_answers(), 1);
    assert!(engine.runs_on(Platform::Linux));
    assert!(!engine.runs_on(Platform::Windows));

    let mut gap = server_json();
    gap["known_gap"] = json!("US-MCP-005 follow-up: input cap");
    let gap = parse_value(&gap);
    assert_eq!(
        gap.map(|case| case.known_gap),
        Ok(Some("US-MCP-005 follow-up: input cap".to_owned()))
    );
}

#[test]
fn a_case_with_an_unknown_field_or_an_escaping_location_is_rejected() {
    let mut root = server_json();
    root["bogus"] = json!(1);
    assert!(matches!(
        parse_value(&root),
        Err(CaseError::UnknownField(_))
    ));

    let mut setup = server_json();
    setup["setup"] = json!({"bogus": 1});
    assert!(matches!(
        parse_value(&setup),
        Err(CaseError::UnknownField(_))
    ));

    let mut expect = server_json();
    expect["expect"] = json!({"bogus": {}});
    assert!(matches!(
        parse_value(&expect),
        Err(CaseError::UnknownField(_))
    ));

    for cwd in ["repo/../other_repo", "repo//x"] {
        let mut escaping = server_json();
        escaping["session"] = json!({"cwd": cwd});
        assert!(
            matches!(parse_value(&escaping), Err(CaseError::Location { .. })),
            "{cwd}"
        );
    }

    let mut placeholder = server_json();
    placeholder["forbidden"] = json!(["{nope}"]);
    assert!(matches!(
        parse_value(&placeholder),
        Err(CaseError::Placeholder { .. })
    ));

    let mut profile = server_json();
    profile["session"] = json!({"env": {"GITRAPTOR_PROFILE_DIR": "/real"}});
    assert!(matches!(
        parse_value(&profile),
        Err(CaseError::Invalid { .. })
    ));
}

#[test]
fn a_server_tier_case_with_engine_setup_is_rejected() {
    let mut enabled = server_json();
    enabled["setup"] = json!({"repo": "enabled"});
    assert!(matches!(parse_value(&enabled), Err(CaseError::Tier(_))));

    let mut agent = server_json();
    agent["session"] = json!({"parent": "agent"});
    assert!(matches!(parse_value(&agent), Err(CaseError::Tier(_))));

    let mut windows = engine_json();
    windows["platforms"] = json!(["macos", "linux", "windows"]);
    windows.as_object_mut().map(|o| o.remove("pending"));
    assert!(matches!(parse_value(&windows), Err(CaseError::Platform(_))));

    let mut partial = server_json();
    partial["platforms"] = json!(["macos", "linux"]);
    assert!(matches!(
        parse_value(&partial),
        Err(CaseError::Invalid { .. })
    ));

    let mut ignored = server_json();
    ignored["expect"] = json!({"ignored": {}});
    assert!(matches!(
        parse_value(&ignored),
        Err(CaseError::Invalid { .. })
    ));
}

#[test]
fn a_traversal_case_that_gets_through_is_not_rejected() {
    let case = cursor_case();

    let through = observation(vec![success(&json!({"repo": "x"}))]);
    assert!(judged(&case, &through).contains(&Failure::NotRejected));

    let rejected = observation(vec![cursor_error()]);
    assert_eq!(
        judge(&case, &rejected, &Secrets::default(), &limits()),
        Verdict::Rejected
    );

    let wrong_field = observation(vec![protocol_error(
        -32602,
        "invalid-params",
        Some(json!({"field": {"untrusted": "label"}})),
    )]);
    assert!(
        judged(&case, &wrong_field)
            .iter()
            .any(|f| matches!(f, Failure::WrongRejection { .. }))
    );
}

#[test]
fn a_refusal_with_an_undeclared_field_breaks_the_allowlist() {
    let case = refusal_case();
    let clean = observation(vec![invalid_text_refusal()]);
    assert_eq!(
        judge(&case, &clean, &Secrets::default(), &limits()),
        Verdict::Rejected
    );

    let extra_key = observation(vec![refusal_text(&json!({
        "code": "invalid-text",
        "message": "The label is not valid text.",
        "action": "Use plain text.",
        "repo_path": "/home/someone/repo"
    }))]);
    assert!(field_not_allowed(&judged(&case, &extra_key)));

    let extra_param = observation(vec![refusal_text(&json!({
        "code": "invalid-text",
        "message": "The label is not valid text.",
        "action": "Use plain text.",
        "params": {"field": "label", "max_chars": 64, "path": "/home/someone"}
    }))]);
    assert!(field_not_allowed(&judged(&case, &extra_param)));

    let error_case = cursor_case();
    let extra_data = observation(vec![protocol_error(
        -32602,
        "invalid-params",
        Some(json!({"field": {"untrusted": "cursor"}, "secret": "x"})),
    )]);
    assert!(field_not_allowed(&judged(&error_case, &extra_data)));
}

#[test]
fn a_success_with_a_field_outside_its_output_schema_breaks_the_allowlist() {
    let case = case_with(vec![status_call()], Expect::InvalidRequest);
    let leaking = observation(vec![success(&json!({"repo": "x", "path": "/etc"}))]);
    assert!(field_not_allowed(&judged(&case, &leaking)));

    let schema = status_tool()["outputSchema"].clone();
    assert!(conforms(&schema, &schema, &json!({"repo": "x"})).is_ok());
    assert!(conforms(&schema, &schema, &json!({"repo": "x", "path": "/etc"})).is_err());

    let unsupported = json!({"type": "object", "unevaluatedProperties": false});
    assert!(conforms(&unsupported, &unsupported, &json!({})).is_err());
}

#[test]
fn a_planted_secret_in_an_answer_or_in_stderr_is_found_without_printing_it() {
    let value = "grc0123456789abcdef0123456789abcdef";
    let secrets = Secrets(vec![("env-github-token".into(), value.into())]);
    let case = cursor_case();

    let mut in_stdout = observation(vec![cursor_error()]);
    in_stdout
        .stdout
        .push(json!({"jsonrpc": "2.0", "method": "x", "params": {"t": value}}).to_string());
    let found = failures(judge(&case, &in_stdout, &secrets, &limits()));
    let leak = Failure::SecretLeaked {
        secret: "env-github-token".into(),
        stream: Stream::Stdout,
    };
    assert!(found.contains(&leak), "{found:?}");
    assert!(found.iter().all(|f| !f.to_string().contains(value)));

    let mut in_stderr = observation(vec![cursor_error()]);
    in_stderr.stderr = format!("raptor-mcp: {value}\n");
    let found = failures(judge(&case, &in_stderr, &secrets, &limits()));
    let leak = Failure::SecretLeaked {
        secret: "env-github-token".into(),
        stream: Stream::Stderr,
    };
    assert!(found.contains(&leak), "{found:?}");
    assert!(found.iter().all(|f| !f.to_string().contains(value)));

    let mut shaped = observation(vec![cursor_error()]);
    shaped.stdout.push(
        json!({"jsonrpc": "2.0", "method": "x", "params": {"t": format!("ghp_{}", "a".repeat(36))}})
            .to_string(),
    );
    assert!(judged(&case, &shaped).iter().any(|f| matches!(
        f,
        Failure::TokenShape {
            stream: Stream::Stdout,
            ..
        }
    )));
}

#[test]
fn a_part_over_its_budget_fails_the_case() {
    let over_part = observation(vec![success(&json!({"repo": "a".repeat(PART_BYTES)}))]);
    let case = case_with(vec![status_call()], Expect::InvalidRequest);
    assert!(
        judged(&case, &over_part)
            .iter()
            .any(|f| matches!(f, Failure::OverBudget { .. }))
    );

    let over_refusal = observation(vec![refusal_text(&json!({
        "code": "invalid-text",
        "message": "x".repeat(REFUSAL_BYTES),
        "action": "Use plain text."
    }))]);
    assert!(
        judged(&refusal_case(), &over_refusal)
            .iter()
            .any(|f| matches!(f, Failure::OverBudget { .. }))
    );
}

#[test]
fn stderr_with_anything_but_fixed_codes_fails_the_case() {
    let case = cursor_case();
    let mut fixed = observation(vec![cursor_error()]);
    fixed.stderr = "raptor-mcp: internal-error\n".into();
    assert_eq!(
        judge(&case, &fixed, &Secrets::default(), &limits()),
        Verdict::Rejected
    );
    assert_eq!(stderr_fixed_codes("raptor-mcp: internal-error\n"), Ok(()));

    let mut panicked = observation(vec![cursor_error()]);
    panicked.stderr = "panicked at src/x.rs\n".into();
    assert!(judged(&case, &panicked).contains(&Failure::StderrNotFixed { line: 1 }));
    assert_eq!(stderr_fixed_codes("panicked at src/x.rs\n"), Err(1));
}

#[test]
fn hidden_characters_in_an_answer_fail_the_case() {
    let case = cursor_case();
    for hidden in ['\u{202e}', '\u{e0041}', '\u{200b}', '\u{1b}'] {
        assert!(is_hidden(hidden), "{hidden:?}");

        let in_value = observation(vec![protocol_error(
            -32602,
            "invalid-params",
            Some(json!({"field": {"untrusted": format!("a{hidden}b")}})),
        )]);
        let found = judged(&case, &in_value);
        assert!(
            found.iter().any(|f| matches!(
                f,
                Failure::HiddenCharacter { at } if at.contains("/error/data/field/untrusted")
            )),
            "{hidden:?} in a value: {found:?}"
        );

        let mut data = serde_json::Map::new();
        data.insert(format!("k{hidden}"), json!(1));
        let in_key = observation(vec![protocol_error(
            -32602,
            "invalid-params",
            Some(Value::Object(data)),
        )]);
        let found = judged(&case, &in_key);
        assert!(
            found
                .iter()
                .any(|f| matches!(f, Failure::HiddenCharacter { .. })),
            "{hidden:?} in a key: {found:?}"
        );
    }
    assert!(!is_hidden('a'));
    assert_eq!(
        hidden_characters(&json!({"a": "x\u{202e}y"})),
        vec!["/a".to_owned()]
    );
}

#[test]
fn a_repo_change_or_a_fired_trap_fails_the_case() {
    let case = cursor_case();

    let mut changed = observation(vec![cursor_error()]);
    changed.repo_changes = vec!["modified .env".into()];
    assert!(
        judged(&case, &changed)
            .iter()
            .any(|f| matches!(f, Failure::RepoChanged(_)))
    );

    let mut trapped = observation(vec![cursor_error()]);
    trapped.traps_fired = vec!["git".into()];
    assert!(
        judged(&case, &trapped)
            .iter()
            .any(|f| matches!(f, Failure::TrapFired(_)))
    );

    let mut died = observation(vec![cursor_error()]);
    died.sentinel_answered = false;
    assert!(judged(&case, &died).contains(&Failure::SessionDied));
}

#[test]
fn the_report_prints_the_kpi_and_the_gate_fails_below_100_percent() {
    let all = Report {
        os: "test".into(),
        rows: vec![
            row("one", Outcome::Rejected),
            row("two", Outcome::Rejected),
            row("three", Outcome::Rejected),
        ],
    };
    assert_eq!(all.kpi_permille(), 1000);
    assert_eq!(all.gate(), Ok(()));
    assert!(
        all.summary_line().contains("3/3 rejected (100.0 %)"),
        "{}",
        all.summary_line()
    );
    assert!(
        all.markdown()
            .starts_with("<!-- mcp-corpus os=test executed=3 rejected=3 pending=0 -->"),
        "{}",
        all.markdown()
    );

    let two_of_three = Report {
        os: "test".into(),
        rows: vec![
            row("one", Outcome::Rejected),
            row("two", Outcome::Rejected),
            row("bad-case", Outcome::Failed(vec![Failure::NotRejected])),
        ],
    };
    assert_eq!(two_of_three.kpi_permille(), 666);
    assert!(
        two_of_three
            .gate()
            .is_err_and(|message| message.contains("bad-case"))
    );
}

#[test]
fn an_empty_corpus_fails_the_gate() {
    assert!(Report::default().gate().is_err());

    let all_pending = Report {
        os: "test".into(),
        rows: vec![row("one", Outcome::Pending("XP-42".into()))],
    };
    assert!(all_pending.gate().is_err());
}

#[test]
fn a_pending_case_is_counted_apart_and_never_as_rejected() {
    let report = Report {
        os: "test".into(),
        rows: vec![
            row("one", Outcome::Rejected),
            row("two", Outcome::Pending("XP-42".into())),
        ],
    };
    assert_eq!(report.executed(), 1);
    assert_eq!(report.rejected(), 1);
    assert_eq!(report.pending(), 1);
    assert_eq!(report.kpi_permille(), 1000);
}

#[test]
fn a_known_gap_case_is_counted_apart_and_never_as_rejected() {
    let report = Report {
        os: "test".into(),
        rows: vec![
            row("one", Outcome::Rejected),
            row("gap", Outcome::KnownGap("US-MCP-005".into())),
            row("two", Outcome::Pending("XP-42".into())),
        ],
    };
    assert_eq!(report.executed(), 1);
    assert_eq!(report.rejected(), 1);
    assert_eq!(report.pending(), 1);
    assert_eq!(report.known_gap(), 1);
    assert_eq!(report.kpi_permille(), 1000);
    assert_eq!(report.gate(), Ok(()));

    // A closed gap: the server now rejects a case that still carries the mark.
    let mut marked = cursor_case();
    marked.known_gap = Some("US-MCP-005".into());
    let rejected = observation(vec![cursor_error()]);
    assert!(judged(&marked, &rejected).contains(&Failure::KnownGapClosed));

    // An open gap: the real answer is a success, so the case stays a known gap.
    let through = observation(vec![success(&json!({"repo": "x"}))]);
    assert_eq!(
        judge(&marked, &through, &Secrets::default(), &limits()),
        Verdict::KnownGap("US-MCP-005".into())
    );
}
