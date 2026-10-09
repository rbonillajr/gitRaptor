//! The judge: the same checks for every case, over what one session produced.

use serde_json::Value;

use super::case::{Case, Expect};
use super::scan;
use super::schema::conforms;

/// Size limits the runner takes from the production crate (the testkit depends on none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub part_bytes: usize,
    pub refusal_bytes: usize,
    pub name_chars: usize,
}

/// Planted secrets as `(name, value)`. Only the name is ever reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secrets(pub Vec<(String, String)>);

/// One answer of the server to a message of the case.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub tool: Option<String>,
    pub message: Value,
}

/// Everything one session produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observation {
    /// `result.tools` of this session's `tools/list`.
    pub tools: Vec<Value>,
    /// In arrival order; handshake, `tools/list` and the sentinel excluded.
    pub answers: Vec<Answer>,
    /// Every stdout line, verbatim, handshake included.
    pub stdout: Vec<String>,
    pub stderr: String,
    pub sentinel_answered: bool,
    /// `Change` (Display) left after the case's exceptions.
    pub repo_changes: Vec<String>,
    pub traps_fired: Vec<String>,
}

/// An output stream of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Why a case was not rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    NotRejected,
    WrongRejection {
        expected: String,
        got: String,
    },
    MissingAnswers {
        expected: usize,
        got: usize,
    },
    SessionDied,
    StdoutNotProtocol {
        line: usize,
    },
    FieldNotAllowed {
        at: String,
    },
    OverBudget {
        what: String,
        bytes: usize,
        limit: usize,
    },
    /// The canary's NAME, never its value.
    SecretLeaked {
        secret: String,
        stream: Stream,
    },
    TokenShape {
        shape: &'static str,
        stream: Stream,
    },
    HiddenCharacter {
        at: String,
    },
    StderrNotFixed {
        line: usize,
    },
    RepoChanged(Vec<String>),
    TrapFired(Vec<String>),
    /// The case carries a `known_gap` mark but the server now rejects it: the gap is closed and
    /// the mark must be dropped so the case demands the rejection from here on.
    KnownGapClosed,
    Harness(String),
}

/// Escapes text that came from a response and cuts it to 120 characters.
fn clip(text: &str, max: usize) -> String {
    let escaped: String = text.chars().flat_map(char::escape_debug).collect();
    if escaped.chars().count() > max {
        let mut cut: String = escaped.chars().take(max).collect();
        cut.push('…');
        cut
    } else {
        escaped
    }
}

fn stream_name(stream: Stream) -> &'static str {
    match stream {
        Stream::Stdout => "stdout",
        Stream::Stderr => "stderr",
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRejected => f.write_str("the attack was not rejected"),
            Self::WrongRejection { expected, got } => write!(
                f,
                "rejected the wrong way: expected {}, got {}",
                clip(expected, 120),
                clip(got, 120)
            ),
            Self::MissingAnswers { expected, got } => {
                write!(f, "expected {expected} answers, got {got}")
            }
            Self::SessionDied => f.write_str("the session died before the sentinel answered"),
            Self::StdoutNotProtocol { line } => {
                write!(f, "stdout line {line} is not a JSON object")
            }
            Self::FieldNotAllowed { at } => write!(f, "field not allowed at {}", clip(at, 120)),
            Self::OverBudget { what, bytes, limit } => {
                write!(f, "{} is {bytes} over a limit of {limit}", clip(what, 120))
            }
            Self::SecretLeaked { secret, stream } => write!(
                f,
                "planted secret {} leaked on {}",
                clip(secret, 120),
                stream_name(*stream)
            ),
            Self::TokenShape { shape, stream } => write!(
                f,
                "a {} token shape appeared on {}",
                clip(shape, 120),
                stream_name(*stream)
            ),
            Self::HiddenCharacter { at } => {
                write!(f, "hidden character at {}", clip(at, 120))
            }
            Self::StderrNotFixed { line } => {
                write!(f, "stderr line {line} is not a fixed code")
            }
            Self::RepoChanged(changes) => {
                write!(f, "the repo changed: {}", clip(&changes.join("; "), 120))
            }
            Self::TrapFired(traps) => {
                write!(f, "a PATH trap fired: {}", clip(&traps.join(", "), 120))
            }
            Self::KnownGapClosed => {
                f.write_str("the server now rejects this known_gap case: drop the known_gap mark")
            }
            Self::Harness(message) => write!(f, "harness: {}", clip(message, 120)),
        }
    }
}

/// The judge's answer for one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Rejected,
    Failed(Vec<Failure>),
    /// A `known_gap` case that got through, with its reference: expected today, counted apart.
    KnownGap(String),
}

type Object = serde_json::Map<String, Value>;

const GOT_CHARS: usize = 80;

fn push_unique(failures: &mut Vec<Failure>, failure: Failure) {
    if !failures.contains(&failure) {
        failures.push(failure);
    }
}

/// Applies the common checks and the case's `expect` to one observation.
pub fn judge(
    case: &Case,
    observation: &Observation,
    secrets: &Secrets,
    limits: &Limits,
) -> Verdict {
    let mut failures = Vec::new();

    // 1. stdout is protocol only, stderr is fixed codes only.
    for (index, line) in observation.stdout.iter().enumerate() {
        if !matches!(serde_json::from_str::<Value>(line), Ok(Value::Object(_))) {
            push_unique(
                &mut failures,
                Failure::StdoutNotProtocol { line: index + 1 },
            );
        }
    }
    if let Err(line) = scan::stderr_fixed_codes(&observation.stderr) {
        push_unique(&mut failures, Failure::StderrNotFixed { line });
    }

    // 2. no planted secret and no known token shape on either stream.
    let streams = observation
        .stdout
        .iter()
        .map(|line| (Stream::Stdout, line.as_str()))
        .chain(std::iter::once((
            Stream::Stderr,
            observation.stderr.as_str(),
        )));
    for (stream, text) in streams {
        for secret in scan::secrets_in(text, secrets) {
            push_unique(&mut failures, Failure::SecretLeaked { secret, stream });
        }
        for shape in scan::token_shapes(text) {
            push_unique(&mut failures, Failure::TokenShape { shape, stream });
        }
    }

    // 3. the session ran to the end and answered every message.
    if !observation.sentinel_answered {
        failures.push(Failure::SessionDied);
    }
    let expected = case.expected_answers();
    if observation.answers.len() != expected {
        failures.push(Failure::MissingAnswers {
            expected,
            got: observation.answers.len(),
        });
    }

    // 4. hidden characters and the field allowlist, answer by answer.
    for (index, answer) in observation.answers.iter().enumerate() {
        check_answer(index, answer, case, observation, limits, &mut failures);
    }

    // 5. the verdict on what the server said.
    check_expectation(case, observation, &mut failures);

    // 6. the machine is untouched.
    if !observation.repo_changes.is_empty() {
        failures.push(Failure::RepoChanged(observation.repo_changes.clone()));
    }
    if !observation.traps_fired.is_empty() {
        failures.push(Failure::TrapFired(observation.traps_fired.clone()));
    }

    // 7. a `known_gap` case is expected to get through: only the expectation may differ.
    if let Some(gap) = &case.known_gap {
        if failures.is_empty() {
            return Verdict::Failed(vec![Failure::KnownGapClosed]);
        }
        let only_expectation = failures
            .iter()
            .all(|f| matches!(f, Failure::NotRejected | Failure::WrongRejection { .. }));
        if only_expectation {
            return Verdict::KnownGap(gap.clone());
        }
    }
    if failures.is_empty() {
        Verdict::Rejected
    } else {
        Verdict::Failed(failures)
    }
}

fn not_allowed(failures: &mut Vec<Failure>, at: String) {
    push_unique(failures, Failure::FieldNotAllowed { at });
}

fn only_keys(object: &Object, allowed: &[&str], at: &str, failures: &mut Vec<Failure>) {
    for key in object.keys().filter(|key| !allowed.contains(&key.as_str())) {
        not_allowed(failures, format!("{at}/{key}"));
    }
}

fn first_text(result: &Object) -> Option<&str> {
    result
        .get("content")?
        .as_array()?
        .first()?
        .get("text")?
        .as_str()
}

fn check_answer(
    index: usize,
    answer: &Answer,
    case: &Case,
    observation: &Observation,
    limits: &Limits,
    failures: &mut Vec<Failure>,
) {
    let at = format!("#{index}");
    for pointer in scan::hidden_characters(&answer.message) {
        push_unique(
            failures,
            Failure::HiddenCharacter {
                at: format!("{at}{pointer}"),
            },
        );
    }
    let Some(top) = answer.message.as_object() else {
        not_allowed(failures, at);
        return;
    };
    if let Some(error) = top.get("error") {
        only_keys(top, &["jsonrpc", "id", "error"], &at, failures);
        check_protocol_error(&at, error, &answer.message, limits, failures);
    } else if let Some(Value::Object(result)) = top.get("result") {
        only_keys(top, &["jsonrpc", "id", "result"], &at, failures);
        let is_refusal = result.get("isError") == Some(&Value::Bool(true));
        if is_refusal {
            check_refusal(&at, result, case, limits, failures);
        } else {
            check_success(&at, answer, result, observation, limits, failures);
        }
    } else {
        not_allowed(failures, format!("{at}/result"));
    }
}

fn check_protocol_error(
    at: &str,
    error: &Value,
    whole: &Value,
    limits: &Limits,
    failures: &mut Vec<Failure>,
) {
    let bytes = whole.to_string().len();
    if bytes > limits.part_bytes {
        failures.push(Failure::OverBudget {
            what: "protocol error".into(),
            bytes,
            limit: limits.part_bytes,
        });
    }
    let at = format!("{at}/error");
    let Some(error) = error.as_object() else {
        not_allowed(failures, at);
        return;
    };
    only_keys(error, &["code", "message", "data"], &at, failures);
    let Some(data) = error.get("data") else {
        return;
    };
    let at = format!("{at}/data");
    let Some(data) = data.as_object() else {
        not_allowed(failures, at);
        return;
    };
    only_keys(data, &["field"], &at, failures);
    let Some(field) = data.get("field") else {
        return;
    };
    let at = format!("{at}/field");
    let Some(field) = field.as_object() else {
        not_allowed(failures, at);
        return;
    };
    only_keys(field, &["untrusted", "truncated"], &at, failures);
    match field.get("untrusted") {
        None => {}
        Some(Value::String(name)) => {
            let chars = name.chars().count();
            if chars > limits.name_chars {
                failures.push(Failure::OverBudget {
                    what: "untrusted field name (chars)".into(),
                    bytes: chars,
                    limit: limits.name_chars,
                });
            }
        }
        Some(_) => not_allowed(failures, format!("{at}/untrusted")),
    }
}

/// The parsed JSON of the single text block, flagging hidden characters hiding inside it.
fn text_json(at: &str, text: &str, failures: &mut Vec<Failure>) -> Option<Value> {
    let Ok(parsed) = serde_json::from_str::<Value>(text) else {
        not_allowed(failures, format!("{at}/result/content/0/text"));
        return None;
    };
    for pointer in scan::hidden_characters(&parsed) {
        push_unique(
            failures,
            Failure::HiddenCharacter {
                at: format!("{at}/result/content/0/text{pointer}"),
            },
        );
    }
    Some(parsed)
}

fn check_content(at: &str, result: &Object, failures: &mut Vec<Failure>) -> Option<String> {
    let blocks = result.get("content").and_then(Value::as_array);
    let Some([block]) = blocks.map(Vec::as_slice) else {
        not_allowed(failures, format!("{at}/result/content"));
        return None;
    };
    let Some(block) = block.as_object() else {
        not_allowed(failures, format!("{at}/result/content/0"));
        return None;
    };
    only_keys(
        block,
        &["type", "text"],
        &format!("{at}/result/content/0"),
        failures,
    );
    if block.get("type").and_then(Value::as_str) != Some("text") {
        not_allowed(failures, format!("{at}/result/content/0/type"));
    }
    let text = first_text(result).map(str::to_owned);
    if text.is_none() {
        not_allowed(failures, format!("{at}/result/content/0/text"));
    }
    text
}

fn check_refusal(
    at: &str,
    result: &Object,
    case: &Case,
    limits: &Limits,
    failures: &mut Vec<Failure>,
) {
    only_keys(
        result,
        &["content", "isError"],
        &format!("{at}/result"),
        failures,
    );
    let Some(text) = check_content(at, result, failures) else {
        return;
    };
    if text.len() > limits.refusal_bytes {
        failures.push(Failure::OverBudget {
            what: "refusal text".into(),
            bytes: text.len(),
            limit: limits.refusal_bytes,
        });
    }
    let Some(parsed) = text_json(at, &text, failures) else {
        return;
    };
    let body = format!("{at}/result/content/0/text");
    let Some(parsed) = parsed.as_object() else {
        not_allowed(failures, body);
        return;
    };
    only_keys(
        parsed,
        &["code", "message", "action", "params"],
        &body,
        failures,
    );
    for required in ["code", "message", "action"] {
        if !parsed.get(required).is_some_and(Value::is_string) {
            not_allowed(failures, format!("{body}/{required}"));
        }
    }
    if let Some(params) = parsed.get("params") {
        let declared: &[String] = match &case.expect {
            Expect::Refusal { params, .. } => params,
            _ => &[],
        };
        match params.as_object() {
            Some(object) => {
                for key in object.keys().filter(|k| !declared.contains(k)) {
                    not_allowed(failures, format!("{body}/params/{key}"));
                }
            }
            None => not_allowed(failures, format!("{body}/params")),
        }
    }
}

fn check_success(
    at: &str,
    answer: &Answer,
    result: &Object,
    observation: &Observation,
    limits: &Limits,
    failures: &mut Vec<Failure>,
) {
    only_keys(
        result,
        &["content", "structuredContent", "isError"],
        &format!("{at}/result"),
        failures,
    );
    let structured = result.get("structuredContent");
    if let Some(text) = check_content(at, result, failures) {
        if text.len() > limits.part_bytes {
            failures.push(Failure::OverBudget {
                what: "success text".into(),
                bytes: text.len(),
                limit: limits.part_bytes,
            });
        }
        if let Some(parsed) = text_json(at, &text, failures)
            && structured.is_some_and(|s| *s != parsed)
        {
            not_allowed(failures, format!("{at}/result/content/0/text"));
        }
    }
    let Some(structured) = structured else {
        not_allowed(failures, format!("{at}/result/structuredContent"));
        return;
    };
    let bytes = structured.to_string().len();
    if bytes > limits.part_bytes {
        failures.push(Failure::OverBudget {
            what: "structured content".into(),
            bytes,
            limit: limits.part_bytes,
        });
    }
    let schema = answer.tool.as_deref().and_then(|name| {
        observation
            .tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|tool| tool.get("outputSchema"))
    });
    match schema {
        None => not_allowed(
            failures,
            format!("{at}/result/structuredContent (no output schema)"),
        ),
        Some(schema) => {
            if let Err(why) = conforms(schema, schema, structured) {
                not_allowed(failures, format!("{at}/result/structuredContent: {why}"));
            }
        }
    }
}

enum Kind {
    Success,
    Refusal(Option<String>),
    ProtocolError {
        code: Option<i64>,
        message: Option<String>,
        field: Option<String>,
        anonymous: bool,
    },
}

fn kind(message: &Value) -> Kind {
    if let Some(error) = message.get("error") {
        let anonymous = message.get("id").is_none_or(Value::is_null);
        return Kind::ProtocolError {
            code: error.get("code").and_then(Value::as_i64),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned),
            field: error
                .pointer("/data/field/untrusted")
                .and_then(Value::as_str)
                .map(str::to_owned),
            anonymous,
        };
    }
    let result = message.get("result");
    if result.and_then(|r| r.get("isError")) == Some(&Value::Bool(true)) {
        let code = result
            .and_then(Value::as_object)
            .and_then(first_text)
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|body| body.get("code").and_then(Value::as_str).map(str::to_owned));
        return Kind::Refusal(code);
    }
    Kind::Success
}

fn describe(kind: &Kind) -> String {
    let text = match kind {
        Kind::Success => "success".to_owned(),
        Kind::Refusal(code) => format!("refusal:{}", code.as_deref().unwrap_or("?")),
        Kind::ProtocolError {
            code: Some(-32600),
            anonymous: true,
            ..
        } => "invalid_request".to_owned(),
        Kind::ProtocolError { code, message, .. } => {
            let code = code.map_or_else(|| "?".to_owned(), |c| c.to_string());
            match message {
                Some(message) => format!("protocol_error:{code}:{message}"),
                None => format!("protocol_error:{code}"),
            }
        }
    };
    clip(&text, GOT_CHARS)
}

fn matches_expect(expect: &Expect, kind: &Kind) -> bool {
    match (expect, kind) {
        (Expect::Refusal { code, .. }, Kind::Refusal(got)) => got.as_deref() == Some(code),
        (
            Expect::ProtocolError {
                code,
                message,
                field,
            },
            Kind::ProtocolError {
                code: got_code,
                message: got_message,
                field: got_field,
                ..
            },
        ) => {
            *got_code == Some(*code)
                && message
                    .as_ref()
                    .is_none_or(|m| got_message.as_ref() == Some(m))
                && field.as_ref().is_none_or(|f| got_field.as_ref() == Some(f))
        }
        (
            Expect::InvalidRequest,
            Kind::ProtocolError {
                code: Some(-32600),
                anonymous: true,
                ..
            },
        ) => true,
        _ => false,
    }
}

fn check_expectation(case: &Case, observation: &Observation, failures: &mut Vec<Failure>) {
    let kinds: Vec<Kind> = observation
        .answers
        .iter()
        .map(|answer| kind(&answer.message))
        .collect();
    if case.expect == Expect::Ignored {
        if let Some(first) = kinds.first() {
            failures.push(Failure::WrongRejection {
                expected: "ignored".into(),
                got: describe(first),
            });
        }
        return;
    }
    let Some(first) = kinds.iter().find(|k| !matches!(k, Kind::Success)) else {
        failures.push(Failure::NotRejected);
        return;
    };
    if !matches_expect(&case.expect, first) {
        let expected = match &case.expect {
            Expect::Refusal { code, .. } => format!("refusal:{code}"),
            Expect::ProtocolError { code, message, .. } => match message {
                Some(message) => format!("protocol_error:{code}:{message}"),
                None => format!("protocol_error:{code}"),
            },
            Expect::InvalidRequest => "invalid_request".to_owned(),
            Expect::Ignored => "ignored".to_owned(),
        };
        failures.push(Failure::WrongRejection {
            expected: clip(&expected, GOT_CHARS),
            got: describe(first),
        });
    }
}
