//! Runner of the MCP security corpus: drives the real `raptor` and `raptor-mcp` binaries, one
//! temporary machine per case, and hands what it saw to the judge in `gitraptor_testkit`.

mod machine;
mod session;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use gitraptor_api::mcp_view::{
    MAX_MCP_NAME_CHARS, MAX_MCP_PART_BYTES, MCP_BYTES_PER_TOKEN, MCP_REFUSAL_TOKENS,
};
use gitraptor_testkit::diff;
use gitraptor_testkit::mcp_corpus::case::Send as Step;
use gitraptor_testkit::mcp_corpus::{
    Case, Failure, Limits, Observation, Outcome, Platform, Report, Row, Secrets, Tier, Verdict,
    expand, judge, load_dir,
};
use gitraptor_testkit::sibling_bin;
use serde_json::{Map, Value};

use machine::{Machine, RAPTOR};
use session::Launch;

/// What one real session produced, and the secrets planted for it.
pub struct CaseRun {
    pub observation: Observation,
    pub secrets: Secrets,
}

/// The folder with the case files.
pub fn cases_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("mcp_corpus")
        .join("cases")
}

/// Loads every case file; errors are rendered `<file>: <error>`.
pub fn load_corpus() -> Result<Vec<Case>, Vec<String>> {
    load_dir(&cases_dir()).map_err(|errors| {
        errors
            .iter()
            .map(|(file, error)| {
                let name = file.file_name().map_or_else(
                    || file.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                format!("{name}: {error}")
            })
            .collect()
    })
}

/// The size limits of the production crate.
pub fn limits() -> Limits {
    Limits {
        part_bytes: MAX_MCP_PART_BYTES,
        refusal_bytes: MCP_REFUSAL_TOKENS * MCP_BYTES_PER_TOKEN,
        name_chars: MAX_MCP_NAME_CHARS,
    }
}

/// `text` with its markers replaced; a marker that cannot be resolved is a harness failure.
fn expanded(text: &str, vars: &BTreeMap<String, String>, what: &str) -> String {
    expand(text, vars).unwrap_or_else(|e| panic!("{what}: {e}"))
}

/// Every string of `value`, keys included, with its markers replaced.
fn expand_json(value: &Value, vars: &BTreeMap<String, String>, what: &str) -> Value {
    match value {
        Value::String(text) => Value::String(expanded(text, vars, what)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| expand_json(v, vars, what)).collect())
        }
        Value::Object(map) => Value::Object(expand_map(map, vars, what)),
        other => other.clone(),
    }
}

fn expand_map(
    map: &Map<String, Value>,
    vars: &BTreeMap<String, String>,
    what: &str,
) -> Map<String, Value> {
    map.iter()
        .map(|(key, value)| (expanded(key, vars, what), expand_json(value, vars, what)))
        .collect()
}

fn expand_step(step: &Step, vars: &BTreeMap<String, String>) -> Step {
    match step {
        Step::Call {
            tool,
            arguments,
            repeat,
        } => Step::Call {
            tool: expanded(tool, vars, "send.call"),
            arguments: arguments
                .as_ref()
                .map(|a| expand_json(a, vars, "send.arguments")),
            repeat: *repeat,
        },
        Step::Raw(line) => Step::Raw(expanded(line, vars, "send.raw")),
        Step::Message(map) => Step::Message(expand_map(map, vars, "send.message")),
    }
}

/// Runs one case in one real session on its own machine.
///
/// # Panics
/// On a harness failure (a setup step that cannot be done, a read that runs out of time).
/// [`run_corpus`] turns that into a failed row.
pub fn run_case(case: &Case) -> CaseRun {
    let machine = Machine::new(case);
    let vars = machine.vars();
    let exceptions = machine.exceptions();
    let steps: Vec<Step> = case
        .send
        .iter()
        .map(|step| expand_step(step, &vars))
        .collect();
    let mut secrets = machine.secrets();
    for (n, text) in case.forbidden.iter().enumerate() {
        secrets
            .0
            .push((format!("forbidden-{n}"), expanded(text, &vars, "forbidden")));
    }
    let mut env = machine.env();
    for (key, value) in &case.session.env {
        env.push((key.clone(), expanded(value, &vars, "session.env").into()));
    }
    let launch = Launch {
        server: sibling_bin(Path::new(RAPTOR), "gitraptor-mcp", "raptor-mcp"),
        cwd: case.session.cwd.resolve(&machine.roots()),
        env,
        agent: machine.agent().map(Path::to_owned),
    };

    let before = machine.f.snapshot(&exceptions);
    let seen = session::run(&launch, &steps);
    let after = machine.f.snapshot(&exceptions);
    let changes = exceptions.filter(&diff(&before, &after), &before, &after);

    let mut observation = seen.observation;
    observation.repo_changes = changes.iter().map(ToString::to_string).collect();
    observation.traps_fired = machine.traps_fired();
    CaseRun {
        observation,
        secrets,
    }
}

/// The report row of one case: pending off its platforms, else one real run judged.
fn row(case: &Case) -> Row {
    let outcome = match Platform::current() {
        Some(platform) if case.runs_on(platform) => {
            record_panics();
            let judged = catch_unwind(AssertUnwindSafe(|| {
                let run = run_case(case);
                judge(case, &run.observation, &run.secrets, &limits())
            }));
            match judged {
                Ok(Verdict::Rejected) => Outcome::Rejected,
                Ok(Verdict::Failed(failures)) => Outcome::Failed(failures),
                Ok(Verdict::KnownGap(reference)) => Outcome::KnownGap(reference),
                Err(panic) => Outcome::Failed(vec![Failure::Harness(panic_message(&panic))]),
            }
        }
        _ => Outcome::Pending(
            case.pending
                .clone()
                .unwrap_or_else(|| "unsupported platform".to_owned()),
        ),
    };
    Row {
        id: case.id.clone(),
        tier: case.tier,
        outcome,
    }
}

thread_local! {
    /// What the panic hook saw on this thread: the message and where it happened.
    static LAST_PANIC: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Records every panic's location and message on its own thread, then runs the hook that was
/// there. The payload alone may not be text, and then it says nothing about where it came from.
fn record_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let backtrace = std::backtrace::Backtrace::force_capture();
            LAST_PANIC.with(|last| *last.borrow_mut() = Some(format!("{info}\n{backtrace}")));
            previous(info);
        }));
    });
}

fn panic_message(panic: &(dyn std::any::Any + core::marker::Send)) -> String {
    let payload = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied());
    let seen = LAST_PANIC.with(|last| last.borrow_mut().take());
    match (payload, seen) {
        (Some(text), _) => text.to_owned(),
        (None, Some(seen)) => format!("the harness panicked without a text payload: {seen}"),
        (None, None) => "the harness panicked without a text payload".to_owned(),
    }
}

/// Runs every case on a pool of at most four threads; rows keep the order of `cases`.
pub fn run_corpus(cases: &[Case]) -> Report {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(4)
        .min(cases.len().max(1));
    let next = AtomicUsize::new(0);
    let rows: Mutex<Vec<(usize, Row)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(case) = cases.get(at) else { return };
                    let done = row(case);
                    rows.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((at, done));
                }
            });
        }
    });
    let mut rows = rows
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    rows.sort_by_key(|(at, _)| *at);
    let report = Report {
        os: Platform::current()
            .map_or("other", Platform::as_str)
            .to_owned(),
        rows: rows.into_iter().map(|(_, row)| row).collect(),
    };
    assert_floor(&report);
    report
}

/// Floors a run must clear, so a filter or a mass `pending`/`known_gap` mark cannot empty the
/// mandatory path. Raise them when cases land; lower them only on purpose.
const MIN_EXECUTED_SERVER: usize = 40;
const MIN_EXECUTED_ENGINE: usize = 15;
const MAX_KNOWN_GAP: usize = 5;

/// # Panics
/// When the run executed fewer cases than the floors above, or carries too many known gaps.
fn assert_floor(report: &Report) {
    let server = report.executed_in(Tier::Server);
    assert!(
        server >= MIN_EXECUTED_SERVER,
        "only {server} server-tier cases executed, the floor is {MIN_EXECUTED_SERVER}"
    );
    if matches!(Platform::current(), Some(Platform::Macos | Platform::Linux)) {
        let engine = report.executed_in(Tier::Engine);
        assert!(
            engine >= MIN_EXECUTED_ENGINE,
            "only {engine} engine-tier cases executed, the floor is {MIN_EXECUTED_ENGINE}"
        );
    }
    let gaps = report.known_gap();
    assert!(
        gaps <= MAX_KNOWN_GAP,
        "{gaps} known_gap cases, the ceiling is {MAX_KNOWN_GAP}"
    );
}

/// Writes the markdown report and returns its path.
pub fn write_report(report: &Report) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        panic!("cannot create {}: {e}", dir.display());
    }
    let path = dir.join("mcp-security-corpus.md");
    if let Err(e) = std::fs::write(&path, report.markdown()) {
        panic!("cannot write {}: {e}", path.display());
    }
    path
}
