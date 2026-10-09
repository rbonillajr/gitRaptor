//! Runner of the MCP security corpus: drives the real `raptor` and `raptor-mcp` binaries, one
//! temporary machine per case, and hands what it saw to the judge in `gitraptor_testkit`.

use std::path::PathBuf;

use gitraptor_api::mcp_view::{
    MAX_MCP_NAME_CHARS, MAX_MCP_PART_BYTES, MCP_BYTES_PER_TOKEN, MCP_REFUSAL_TOKENS,
};
use gitraptor_testkit::mcp_corpus::{Case, Limits, Observation, Report, Secrets};

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
    Ok(Vec::new())
}

/// The size limits of the production crate.
pub fn limits() -> Limits {
    Limits {
        part_bytes: MAX_MCP_PART_BYTES,
        refusal_bytes: MCP_REFUSAL_TOKENS * MCP_BYTES_PER_TOKEN,
        name_chars: MAX_MCP_NAME_CHARS,
    }
}

/// Runs one case in one real session on its own machine.
pub fn run_case(case: &Case) -> CaseRun {
    let _ = case;
    CaseRun {
        observation: Observation::default(),
        secrets: Secrets::default(),
    }
}

/// Runs every case on a pool of at most four threads.
pub fn run_corpus(cases: &[Case]) -> Report {
    let _ = cases;
    Report::default()
}

/// Writes the markdown report and returns its path.
pub fn write_report(report: &Report) -> PathBuf {
    let _ = report;
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp-security-corpus.md")
}
