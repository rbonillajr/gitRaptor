//! Named cut points of the Guardrails transactions (INF-GRD-001, NFR-12): the protocol of
//! `gitraptor_testkit::cut`, behind the cargo feature `test-cuts` and never in a release build.
//! With `GITRAPTOR_TEST_CUT_TRACE` the process appends `<step>:<when>` for every point it
//! passes; with `GITRAPTOR_TEST_CUT=<step>:<when>` it dies at that point with exit code 86,
//! without cleanup. Without the feature every point is a no-op.

/// Where in a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Before,
    After,
}

#[cfg(feature = "test-cuts")]
pub fn trip(step: &str, when: When) {
    let point = format!(
        "{step}:{}",
        match when {
            When::Before => "before",
            When::After => "after",
        }
    );
    if let Some(trace) = std::env::var_os("GITRAPTOR_TEST_CUT_TRACE") {
        use std::io::Write;
        if let Ok(mut file) = std::fs::File::options()
            .create(true)
            .append(true)
            .open(trace)
        {
            let _ = writeln!(file, "{point}");
        }
    }
    if std::env::var("GITRAPTOR_TEST_CUT").is_ok_and(|cut| cut.trim() == point) {
        std::process::exit(86);
    }
}

#[cfg(not(feature = "test-cuts"))]
#[inline(always)]
pub fn trip(_step: &str, _when: When) {}
