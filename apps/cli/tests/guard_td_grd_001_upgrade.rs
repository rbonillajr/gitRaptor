//! The in-place upgrade of the dispatchers to template 3, end to end (ADR-GRD-001 § 8): an install
//! of template 1 or 2 keeps working until it is installed again, the upgrade needs no repair
//! screen, and killing the daemon at each point of it leaves a working dispatcher that the next
//! `raptor guard install` completes. Real `raptor`, real `raptor-hook` and plain Git over the
//! temporary machine of `td_grd_001_machine` (NFR-01). The daemon dies at the point (the `chaos`
//! feature the CLI's tests build `gitraptor-core` with). No fixed waits.
//!
//! Unix only; Windows has no channel transport yet (Pendiente: etapa de validación
//! multiplataforma).
#![cfg(unix)]

mod guard_machine;
mod td_grd_001_machine;

use gitraptor_api::guard::{Diagnostic, ProtectionState};
use gitraptor_testkit::cut::{CutPoint, ENV_CUT, ENV_TRACE, points};
use guard_machine::{install_exceptions, uninstalled_exceptions};
use td_grd_001_machine::{AGENT_PATHS, FORBIDDEN, Td, hooks_of, text};

#[test]
fn fake_agent_entry() {
    td_grd_001_machine::fake_agent_entry();
}

/// The steps of the upgrade, in order, each with its `before` and `after` point.
const STEPS: &[&str] = &[
    "upgrade-journal",
    "upgrade-first-dispatcher",
    "upgrade-other-dispatchers",
    "upgrade-conf",
    "upgrade-manifest",
    "upgrade-commit",
];

const REPAIR_SCREEN: &str = "Repair the protection";

fn outdated(m: &Td) -> bool {
    m.status(&m.repo())
        .diagnostics
        .contains(&Diagnostic::TemplateOutdated)
}

/// The agent's push of a forbidden path to a tag, which template 3 denies.
fn assert_tag_push_denied(m: &Td, bad: &str, why: &str) {
    let out = m.agent_push(&format!("{bad}:refs/tags/denied"));
    assert!(!out.status.success(), "{why}: {}", text(&out));
    assert!(text(&out).contains(FORBIDDEN), "{why}: {}", text(&out));
    assert_eq!(m.remote_ref("refs/tags/denied"), None, "{why}");
}

// Ficha: an install of template 1 or 2 keeps working without reinstalling.
#[test]
fn repo_intact_templates_1_and_2_keep_working_until_reinstalled() {
    for template in [1, 2] {
        let m = Td::new(Some(AGENT_PATHS), true);
        let bad = m.commit_on("bad", FORBIDDEN);
        m.downgrade_to(template);

        let status = m.status(&m.repo());
        assert_eq!(
            status.state,
            ProtectionState::HooksOnly,
            "template {template}"
        );
        assert!(
            status.diagnostics.contains(&Diagnostic::TemplateOutdated),
            "template {template}: {:?}",
            status.diagnostics
        );
        assert_eq!(m.conf_template(), template.to_string());

        // The minimum still holds.
        m.git_ok(&m.repo(), &["switch", "-q", "-c", "side"]);
        let out = m.git(&m.repo(), &["branch", "-D", "main"]);
        assert!(!out.status.success(), "template {template}: {}", text(&out));
        // A push of only a tag is not handed over, as before.
        let out = m.agent_push(&format!("{bad}:refs/tags/x"));
        assert!(out.status.success(), "template {template}: {}", text(&out));
        assert!(m.remote_ref("refs/tags/x").is_some(), "template {template}");
    }
}

/// `raptor guard install` over an install of `from`: no repair screen, template 3 in the
/// constants, every dispatcher in place, no note about an older template, the minimum holds and
/// the forbidden path to a tag is denied.
fn upgrade_from(from: u32) {
    let m = Td::new(Some(AGENT_PATHS), true);
    let bad = m.commit_on("bad", FORBIDDEN);
    m.downgrade_to(from);
    assert!(outdated(&m), "the simulated install is not outdated");

    let out = m.protect(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains(REPAIR_SCREEN), "{}", text(&out));
    assert_eq!(m.conf_template(), "3");
    for hook in hooks_of(3) {
        assert!(m.folder().join("hooks").join(hook).is_file(), "{hook}");
    }
    let status = m.status(&m.repo());
    assert_eq!(status.state, ProtectionState::HooksOnly);
    assert!(
        !status.diagnostics.contains(&Diagnostic::TemplateOutdated),
        "{:?}",
        status.diagnostics
    );
    assert_tag_push_denied(&m, &bad, "after the upgrade");
    m.git_ok(&m.repo(), &["switch", "-q", "-c", "side"]);
    let out = m.git(&m.repo(), &["branch", "-D", "main"]);
    assert!(!out.status.success(), "{}", text(&out));
}

#[test]
fn repo_intact_a_template_1_install_is_upgraded_to_3() {
    upgrade_from(1);
}

#[test]
fn repo_intact_a_template_2_install_is_upgraded_to_3() {
    upgrade_from(2);
}

/// What must hold after the daemon died at `point`: a working dispatcher of the starting
/// template, and the next install completes the upgrade. Problems are returned, not asserted, so
/// a sweep reports every point.
fn after_the_cut(m: &Td, from: u32, point: &CutPoint, bad: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut note = |why: String| problems.push(format!("{point}: {why}"));

    // (a) The dispatchers of the starting template are regular executable files.
    for hook in hooks_of(from) {
        let path = m.folder().join("hooks").join(hook);
        let ok = std::fs::symlink_metadata(&path).is_ok_and(|meta| {
            use std::os::unix::fs::PermissionsExt;
            meta.is_file() && meta.permissions().mode() & 0o111 != 0
        });
        if !ok {
            note(format!("{hook} is not an executable regular file"));
        }
    }
    // (b) Active, with no cause of loss.
    let status = m.status(&m.repo());
    if status.state != ProtectionState::HooksOnly {
        note(format!("state {:?}", status.state));
    }
    if let Some(cause) = status.hooks.as_ref().and_then(|h| h.cause) {
        note(format!("loss cause {cause:?}"));
    }
    // (c) It works: the minimum holds and a clean push goes ahead, with no complaint about the
    // hooks.
    let repo = m.repo();
    let _ = m.git(&repo, &["switch", "-q", "-c", "side"]);
    let denied = m.git(&repo, &["branch", "-D", "main"]);
    if denied.status.success() {
        note("branch -D main passed".into());
    }
    let pushed = m.git(&repo, &["push", "-q", "origin", "side"]);
    if !pushed.status.success() {
        note(format!("a clean push failed: {}", text(&pushed)));
    }
    for shown in [text(&denied), text(&pushed)] {
        for broken in ["moved or altered", "hooks it already had did not run"] {
            if shown.contains(broken) {
                note(format!("the hooks complained: {shown}"));
            }
        }
    }
    // (d) The next install completes the upgrade, with no repair screen. After the last point
    // the upgrade is already confirmed and there is nothing left to do.
    if outdated(m) {
        let out = m.protect(&repo);
        if !out.status.success() || text(&out).contains(REPAIR_SCREEN) {
            note(format!("the next install: {}", text(&out)));
        }
    }
    if m.conf_template() != "3" {
        note(format!(
            "template {} after the next install",
            m.conf_template()
        ));
    }
    if outdated(m) {
        note("still outdated after the next install".into());
    }
    let out = m.agent_push(&format!("{bad}:refs/tags/denied"));
    if out.status.success() || m.remote_ref("refs/tags/denied").is_some() {
        note(format!("a forbidden path reached a tag: {}", text(&out)));
    }
    // (e) No temporary is left.
    let left = m.temporaries();
    if !left.is_empty() {
        note(format!("temporaries: {left:?}"));
    }
    problems
}

/// An install of `from` with a bad commit ready, its daemon stopped.
fn machine_from(from: u32) -> (Td, String) {
    let m = Td::new(Some(AGENT_PATHS), true);
    let bad = m.commit_on("bad", FORBIDDEN);
    m.downgrade_to(from);
    (m, bad)
}

fn sweep(from: u32) {
    // The uncut run passes every declared point, and only those, in order.
    let (m, _) = machine_from(from);
    let trace = m.f.root.join("cut-trace");
    m.with_env(ENV_TRACE, trace.clone());
    let out = m.protect(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    let reached: Vec<String> = std::fs::read_to_string(&trace)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    let declared: Vec<String> = points(STEPS, &[]).iter().map(ToString::to_string).collect();
    assert_eq!(reached, declared);
    drop(m);

    let mut broken = Vec::new();
    for point in points(STEPS, &[]) {
        let (m, bad) = machine_from(from);
        m.with_env(ENV_CUT, point.to_string());
        let out = m.protect(&m.repo());
        assert!(!out.status.success(), "{point}: {}", text(&out));
        m.clear_env();
        // The daemon died at the point; this start serves again.
        let out = m.raptor(&["daemon", "status"]);
        assert!(out.status.success(), "{point}: {}", text(&out));
        broken.extend(after_the_cut(&m, from, &point, &bad));
    }
    assert!(broken.is_empty(), "{broken:#?}");
}

// NFR-12: a cut at any of the 12 points leaves a dispatcher that works, and the next install
// completes the upgrade.
#[test]
fn repo_intact_an_upgrade_from_2_cut_at_any_point_keeps_a_working_dispatcher() {
    sweep(2);
}

#[test]
fn repo_intact_an_upgrade_from_1_cut_at_any_point_keeps_a_working_dispatcher() {
    sweep(1);
}

// NFR-01: what an interrupted upgrade wrote is in the journal, so the uninstall removes all of
// it: no `pre-commit` or `commit-msg` is left behind.
#[test]
fn repo_intact_an_uninstall_after_an_interrupted_upgrade_leaves_no_trace() {
    let m = Td::new(Some(AGENT_PATHS), false);
    let exceptions = uninstalled_exceptions();
    let before = m.f.snapshot(&exceptions);
    let config = std::fs::read(m.common().join("config")).unwrap();
    let out = m.protect(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    m.downgrade_to(1);

    let point = CutPoint::new(
        "upgrade-other-dispatchers",
        gitraptor_testkit::cut::When::After,
    );
    m.with_env(ENV_CUT, point.to_string());
    let out = m.protect(&m.repo());
    assert!(!out.status.success(), "{}", text(&out));
    m.clear_env();
    let out = m.raptor(&["daemon", "status"]);
    assert!(out.status.success(), "{}", text(&out));
    // The dispatchers of the new template are in the folder, not yet confirmed.
    assert!(m.folder().join("hooks/pre-commit").is_file());

    let out = m.uninstall(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    assert!(!m.folder().exists());
    let after = m.f.snapshot(&exceptions);
    let changes = exceptions.filter(&gitraptor_testkit::diff(&before, &after), &before, &after);
    assert!(changes.is_empty(), "{changes:#?}");
    assert_eq!(std::fs::read(m.common().join("config")).unwrap(), config);
}

// M4: the temporary of a write killed between the temporary and the rename is removed by the
// next install and by the uninstall.
#[test]
fn repo_intact_a_temporary_of_a_killed_write_is_cleaned_by_the_next_install() {
    let m = Td::new(Some(AGENT_PATHS), false);
    let exceptions = uninstalled_exceptions();
    let before = m.f.snapshot(&exceptions);
    let out = m.protect(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    m.downgrade_to(2);

    let point = CutPoint::new("upgrade-journal", gitraptor_testkit::cut::When::After);
    m.with_env(ENV_CUT, point.to_string());
    let out = m.protect(&m.repo());
    assert!(!out.status.success(), "{}", text(&out));
    m.clear_env();
    // What a process killed between writing the temporary and renaming it leaves.
    let leftover = m
        .folder()
        .join("hooks/pre-push.gitraptor.tmp-0123456789abcdef");
    std::fs::write(&leftover, b"half a dispatcher").unwrap();
    let out = m.raptor(&["daemon", "status"]);
    assert!(out.status.success(), "{}", text(&out));

    let out = m.protect(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    assert!(!leftover.exists(), "the temporary stays");
    assert!(m.temporaries().is_empty(), "{:?}", m.temporaries());

    let out = m.uninstall(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    let after = m.f.snapshot(&exceptions);
    let changes = exceptions.filter(&gitraptor_testkit::diff(&before, &after), &before, &after);
    assert!(changes.is_empty(), "{changes:#?}");
}

// NFR-01: an upgrade changes the key, the folder of the dispatchers and the profile (the
// journal), and nothing else of the machine.
#[test]
fn repo_intact_an_upgrade_changes_only_the_dispatchers_and_the_journal() {
    let m = Td::new(Some(AGENT_PATHS), false);
    // `downgrade_to` stops the daemon and the next `protect` starts it again: its channel
    // socket under `profile/run` is runtime state, not footprint of the install.
    let exceptions = install_exceptions().with(gitraptor_testkit::Exception::Subtree {
        scope: "profile".into(),
        prefix: "run".into(),
    });
    let before = m.f.snapshot(&exceptions);
    let out = m.protect(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    m.downgrade_to(2);
    let out = m.protect(&m.repo());
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(m.conf_template(), "3");

    let after = m.f.snapshot(&exceptions);
    let changes = exceptions.filter(&gitraptor_testkit::diff(&before, &after), &before, &after);
    assert!(changes.is_empty(), "{changes:#?}");
}
