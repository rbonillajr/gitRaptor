//! INF-GRP-001, dynamic `exec` audit of the read layer (ADR-GRP-009, Validación 5 and 7; SEC-09;
//! M1). A `harness = false` target: the same binary is the test runner, the probe that runs the
//! read layer, and the shim and traps of `gitraptor_testkit::exec_audit` (see that module).
//!
//! - Trap gate (every OS, no privileges): every Git the layer launches goes through the shim and
//!   is checked against the allowlist; no program is launched by name (`gix` never launches
//!   `git`, Git never reaches a pager or `gpg`).
//! - Kernel tracer (deep audit): strace on Linux, eslogger on macOS with root, chosen by
//!   `GITRAPTOR_EXEC_AUDIT`. Skipped when none is available and none was asked for.

mod common;

use std::ffi::OsStr;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use common::{assert_allowlisted, invoker_for, read_everything_with, system_git};
use gitraptor_git::Invoker;
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_testkit::exec_audit::{self, ProbeSpec, Tracer, TrapAudit};
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};

const PROBE: &str = "GITRAPTOR_EXEC_PROBE";
const PROBE_REPO: &str = "GITRAPTOR_EXEC_PROBE_REPO";
const PROBE_SHIM: &str = "GITRAPTOR_EXEC_PROBE_SHIM";

fn main() -> ExitCode {
    if let Some(code) = exec_audit::dispatch() {
        return code;
    }
    if let Ok(mode) = std::env::var(PROBE) {
        return probe(&mode);
    }
    run_tests(&[
        (
            "repo_intact::exec_audit::trap_gate_on_reads",
            trap_gate_on_reads,
        ),
        (
            "repo_intact::exec_audit::detects_launch_by_name",
            detects_launch_by_name,
        ),
        (
            "repo_intact::exec_audit::detects_argv_outside_allowlist",
            detects_argv_outside_allowlist,
        ),
        ("repo_intact::exec_audit::kernel_tracer", kernel_tracer),
    ])
}

// --- Probe: runs in the child, with `HOME`, `PATH=trap/` and nothing else ----------------------

fn probe(mode: &str) -> ExitCode {
    let repo = PathBuf::from(std::env::var_os(PROBE_REPO).expect("probe repo"));
    let shim = PathBuf::from(std::env::var_os(PROBE_SHIM).expect("probe shim"));
    match mode {
        "reads" => {
            let home = std::env::var_os("HOME").expect("HOME");
            let path = std::env::var_os("PATH").expect("PATH");
            let config = ResolveConfig {
                configured_path: Some(shim.clone()),
                path_env: Some(path.clone()),
                known_locations: Vec::new(),
                shim_paths: Vec::new(),
                toolchain_gits: Vec::new(),
            };
            let git = match resolve::resolve(&config, &Invoker::default()) {
                Resolution::Found { git, .. } => git,
                other => panic!("the shim must resolve: {other:?}"),
            };
            assert_eq!(git.path, shim.canonicalize().unwrap());
            read_everything_with(&git, &invoker_for(Path::new(&home), &path), &repo);
        }
        // Negative controls: what the gate must catch.
        "rogue-by-name" => {
            let _ = std::process::Command::new("git").arg("version").output();
        }
        "rogue-argv" => {
            let _ = std::process::Command::new(&shim)
                .args(["status", "--porcelain"])
                .current_dir(&repo)
                .output();
        }
        other => panic!("unknown probe {other}"),
    }
    ExitCode::SUCCESS
}

// --- Parent side --------------------------------------------------------------------------------

struct Setup {
    f: Fixture,
    audit: TrapAudit,
    #[cfg(unix)]
    canary_markers: PathBuf,
}

/// The SEC-09 canary on unix (every configurable program armed), a busy repo elsewhere.
fn setup() -> Setup {
    let git = system_git();
    #[cfg(unix)]
    let (f, canary_markers) = {
        let c = gitraptor_testkit::canary::Canary::arm(Fixture::busy(&git.path));
        let markers = c.markers.clone();
        (c.f, markers)
    };
    #[cfg(not(unix))]
    let f = Fixture::busy(&git.path);
    let audit = TrapAudit::install(&f.root.join("audit"), &git.path);
    Setup {
        f,
        audit,
        #[cfg(unix)]
        canary_markers,
    }
}

fn probe_spec(s: &Setup, mode: &str) -> ProbeSpec {
    let trap = s.audit.trap_path();
    let shim = s.audit.shim_git();
    ProbeSpec::current_exe(
        &[],
        &[
            (PROBE, OsStr::new(mode)),
            (PROBE_REPO, s.f.repo.as_os_str()),
            (PROBE_SHIM, shim.as_os_str()),
            ("HOME", s.f.home.as_os_str()),
            ("PATH", trap.as_os_str()),
        ],
        &s.f.repo,
    )
}

/// What the audit itself writes (its log and markers) is not the engine's.
fn exceptions() -> Exceptions {
    Exceptions::engine_profile("profile").with(Exception::Subtree {
        scope: "audit".into(),
        prefix: "".into(),
    })
}

fn trap_gate_on_reads() {
    let s = setup();
    let spec = probe_spec(&s, "reads");
    check("exec audit: reads under traps", &s.f, &exceptions(), || {
        let out = spec.run();
        assert!(out.success, "probe failed:\n{}\n{}", out.stdout, out.stderr);
    })
    .assert_intact();
    assert!(
        s.audit.fired().is_empty(),
        "launched by name: {:?}",
        s.audit.fired()
    );
    #[cfg(unix)]
    assert!(
        std::fs::read_dir(&s.canary_markers)
            .unwrap()
            .next()
            .is_none(),
        "a configured program ran"
    );
    let launches = s.audit.launches();
    // `version` from resolution, then the 8 CLI reads of `read_everything_with`.
    assert!(launches.len() >= 9, "{launches:?}");
    for argv in &launches {
        assert_allowlisted(argv);
    }
}

fn detects_launch_by_name() {
    let s = setup();
    let out = probe_spec(&s, "rogue-by-name").run();
    assert!(out.success, "{}", out.stderr);
    assert_eq!(
        s.audit.fired(),
        [format!("git{}", std::env::consts::EXE_SUFFIX)
            .trim_end_matches(".exe")
            .to_string()]
    );
}

fn detects_argv_outside_allowlist() {
    let s = setup();
    let out = probe_spec(&s, "rogue-argv").run();
    assert!(out.success, "{}", out.stderr);
    let launches = s.audit.launches();
    assert_eq!(launches, [vec!["status".to_string(), "--porcelain".into()]]);
    let caught = catch_unwind(|| assert_allowlisted(&launches[0]));
    assert!(caught.is_err(), "`status` passed the allowlist check");
}

fn kernel_tracer() {
    let tracer = match Tracer::from_env() {
        Ok(Some(t)) => t,
        Ok(None) => {
            println!("    (no kernel tracer here: set GITRAPTOR_EXEC_AUDIT=strace|eslogger)");
            return;
        }
        Err(e) => panic!("kernel tracer requested but unavailable: {e}"),
    };
    let s = setup();
    let work = s.f.root.join("audit");
    let (out, execs) = tracer
        .trace(&probe_spec(&s, "reads"), &work)
        .unwrap_or_else(|e| panic!("{tracer:?}: {e}"));
    assert!(out.success, "probe failed:\n{}\n{}", out.stdout, out.stderr);
    let git = system_git();
    let allowed = allowed_programs(&git.path, &s.audit.shim_git());
    let trap_dir = PathBuf::from(s.audit.trap_path());
    let mut offenders = Vec::new();
    for e in &execs {
        let program = Path::new(&e.program);
        if program.starts_with(&trap_dir) || !allowed.iter().any(|a| program.starts_with(a)) {
            offenders.push(format!("{} {:?} (ok: {})", e.program, e.argv, e.ok));
        }
    }
    assert!(!execs.is_empty(), "{tracer:?} saw no exec at all");
    assert!(offenders.is_empty(), "unexpected exec: {offenders:#?}");
}

/// The shim, the real Git (and its canonical path and `exec-path`), and on macOS the developer
/// toolchain that `/usr/bin/git` dispatches to.
fn allowed_programs(git: &Path, shim: &Path) -> Vec<PathBuf> {
    let mut out = vec![shim.to_owned(), git.to_owned()];
    if let Ok(c) = git.canonicalize() {
        out.push(c);
    }
    if let Ok(o) = std::process::Command::new(git).arg("--exec-path").output() {
        let p = String::from_utf8_lossy(&o.stdout).trim().to_owned();
        if !p.is_empty() {
            out.push(PathBuf::from(p));
        }
    }
    if cfg!(target_os = "macos") {
        out.extend(
            [
                "/usr/bin/xcrun",
                "/Library/Developer/CommandLineTools/",
                "/Applications/Xcode.app/",
            ]
            .map(PathBuf::from),
        );
    }
    out
}

// --- Minimal runner, libtest-like output; honors name filters ------------------------------------
// Speaks enough of the libtest protocol for `cargo nextest`: `--list --format terse` (and the
// `--ignored` listing, always empty), `--exact` and `--nocapture`.

fn run_tests(tests: &[(&str, fn())]) -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    if flag("--list") {
        if !flag("--ignored") {
            for (name, _) in tests {
                println!("{name}: test");
            }
        }
        return ExitCode::SUCCESS;
    }
    let exact = flag("--exact");
    let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let selected: Vec<_> = tests
        .iter()
        .filter(|(name, _)| {
            filters.is_empty()
                || filters.iter().any(|f| {
                    if exact {
                        name == f
                    } else {
                        name.contains(f.as_str())
                    }
                })
        })
        .collect();
    println!("\nrunning {} tests", selected.len());
    let mut failed = Vec::new();
    for (name, test) in &selected {
        let result = catch_unwind(AssertUnwindSafe(test));
        println!(
            "test {name} ... {}",
            if result.is_ok() { "ok" } else { "FAILED" }
        );
        if result.is_err() {
            failed.push(*name);
        }
    }
    println!(
        "\ntest result: {}. {} passed; {} failed; 0 ignored; 0 measured; {} filtered out\n",
        if failed.is_empty() { "ok" } else { "FAILED" },
        selected.len() - failed.len(),
        failed.len(),
        tests.len() - selected.len()
    );
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
