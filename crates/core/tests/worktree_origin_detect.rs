//! US-WTL-005 (T003): read-only detection of who created a linked worktree.
//! Real folders only under a temporary directory (NFR-01). `OriginEnv` is
//! built by hand: no test touches `HOME` or `CODEX_HOME` of the process.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use gitraptor_api::UntrustedName;
use gitraptor_api::messages::{
    ChangeCounts, DivergenceView, HeadView, UnavailableReason, WorktreeStatus, WorktreeView,
};
use gitraptor_api::worktree_origin::{HandsOffReasonView, OriginEvidenceView, OriginToolView};
use gitraptor_core::worktree_origin::{
    OriginEnv, detect, hands_off, is_locked, origin_of, refuses_destructive,
};
use serde_json::json;

use OriginEvidenceView as Ev;
use OriginToolView as Tool;

struct Land {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    main_root: PathBuf,
    common: PathBuf,
}

/// `<base>/proj` with its `.git`, and an administration directory `wt-a`.
fn land() -> Land {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let main_root = base.join("proj");
    let common = main_root.join(".git");
    std::fs::create_dir_all(common.join("worktrees/wt-a")).unwrap();
    Land {
        _tmp: tmp,
        base,
        main_root,
        common,
    }
}

fn env(homes: &[&Path], codex_home: Option<&Path>) -> OriginEnv {
    OriginEnv {
        homes: homes.iter().map(|p| p.to_path_buf()).collect(),
        codex_home: codex_home.map(Path::to_path_buf),
    }
}

fn no_env() -> OriginEnv {
    env(&[], None)
}

fn found(l: &Land, root: &Path, env: &OriginEnv) -> (Tool, Ev) {
    detect(&l.common, root, Some("wt-a"), false, env)
}

fn ready(head: HeadView) -> WorktreeStatus {
    WorktreeStatus::Ready {
        head,
        counts: ChangeCounts::default(),
        changes: Vec::new(),
        divergence: DivergenceView::BaseMissing,
    }
}

fn branch() -> HeadView {
    HeadView::Branch {
        name: UntrustedName::new("feat"),
    }
}

fn missing() -> WorktreeStatus {
    WorktreeStatus::Unavailable {
        reason: UnavailableReason::Missing,
    }
}

#[test]
fn registered_by_gitraptor_wins_and_is_reserved() {
    let l = land();
    assert_eq!(
        detect(&l.common, &l.base.join("x"), Some("wt-a"), true, &no_env()),
        (Tool::Gitraptor, Ev::Registered)
    );
}

#[test]
fn orca_admin_marker_is_certain_and_beats_the_path() {
    let l = land();
    std::fs::create_dir(l.common.join("worktrees/wt-a/orca")).unwrap();
    // The root also looks like worktrunk's: the mark wins.
    assert_eq!(
        found(&l, &l.base.join("proj.feat"), &no_env()),
        (Tool::Orca, Ev::AdminMarker),
        "a real `orca/` folder in the administration directory is orca/admin-marker, before any path rule"
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_orca_mark_is_not_evidence() {
    let l = land();
    let elsewhere = l.base.join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, l.common.join("worktrees/wt-a/orca")).unwrap();
    assert_eq!(
        found(&l, &l.base.join("anywhere/x"), &no_env()),
        (Tool::Unknown, Ev::None),
        "an `orca` symlink can be written by an agent (SEC-11): it is not a mark"
    );
}

#[test]
fn an_orca_file_is_not_a_mark_either() {
    let l = land();
    std::fs::write(l.common.join("worktrees/wt-a/orca"), "x").unwrap();
    assert_eq!(
        found(&l, &l.base.join("anywhere/x"), &no_env()),
        (Tool::Unknown, Ev::None),
        "`orca` must be a directory"
    );
}

#[test]
fn an_admin_name_that_is_not_one_component_is_not_trusted() {
    let l = land();
    std::fs::create_dir_all(l.common.join("evil/orca")).unwrap();
    for id in ["../evil", "a/b", "", ".."] {
        assert_eq!(
            detect(
                &l.common,
                &l.base.join("anywhere/x"),
                Some(id),
                false,
                &no_env()
            ),
            (Tool::Unknown, Ev::None),
            "admin name {id:?} must not be followed"
        );
    }
    assert_eq!(
        detect(
            &l.common,
            &l.base.join("anywhere/x"),
            None,
            false,
            &no_env()
        ),
        (Tool::Unknown, Ev::None),
        "without an admin name there is no mark to look for"
    );
}

#[test]
fn worktrunk_is_a_sibling_named_repo_dot_something() {
    let l = land();
    let likely = (Tool::Worktrunk, Ev::PathPattern);
    assert_eq!(found(&l, &l.base.join("proj.feat"), &no_env()), likely);
    // It does not need to exist.
    assert_eq!(
        found(&l, &l.base.join("proj.gone-branch"), &no_env()),
        likely
    );
    let unknown = (Tool::Unknown, Ev::None);
    assert_eq!(
        found(&l, &l.base.join("proj."), &no_env()),
        unknown,
        "empty suffix"
    );
    assert_eq!(found(&l, &l.base.join("projfeat"), &no_env()), unknown);
    assert_eq!(found(&l, &l.base.join("other.feat"), &no_env()), unknown);
    assert_eq!(
        found(&l, &l.base.join("sub/proj.feat"), &no_env()),
        unknown,
        "not a sibling"
    );
    assert_eq!(
        found(&l, &l.base.join("proj-feat"), &no_env()),
        unknown,
        "a custom template"
    );
}

#[test]
fn claude_code_lives_under_the_main_root() {
    let l = land();
    let root = l.main_root.join(".claude/worktrees/fix-1");
    assert_eq!(
        found(&l, &root, &no_env()),
        (Tool::ClaudeCode, Ev::PathPattern)
    );
    // A text prefix is not a path prefix.
    let near = l.main_root.join(".claude/worktrees-old/fix-1");
    assert_eq!(found(&l, &near, &no_env()), (Tool::Unknown, Ev::None));
}

#[test]
fn codex_uses_codex_home_or_the_default_under_home() {
    let l = land();
    let home = l.base.join("home");
    let custom = l.base.join("custom-codex");
    let likely = (Tool::Codex, Ev::PathPattern);
    // With CODEX_HOME.
    let e = env(&[&home], Some(&custom));
    assert_eq!(found(&l, &custom.join("worktrees/a1b2/demo"), &e), likely);
    // Without it, `<home>/.codex`.
    let e = env(&[&home], None);
    assert_eq!(
        found(&l, &home.join(".codex/worktrees/a1b2/demo"), &e),
        likely
    );
    // With CODEX_HOME set, the default location no longer counts.
    let e = env(&[&home], Some(&custom));
    assert_eq!(
        found(&l, &home.join(".codex/worktrees/a1b2/demo"), &e),
        (Tool::Unknown, Ev::None)
    );
    // Not under `worktrees/`.
    let e = env(&[&home], None);
    assert_eq!(
        found(&l, &home.join(".codex/other/a1b2/demo"), &e),
        (Tool::Unknown, Ev::None)
    );
}

#[test]
fn orca_path_is_under_home_orca_workspaces() {
    let l = land();
    let home = l.base.join("home");
    let e = env(&[&home], None);
    assert_eq!(
        found(&l, &home.join("orca/workspaces/gitRaptor/feat"), &e),
        (Tool::Orca, Ev::PathPattern)
    );
    assert_eq!(
        found(&l, &home.join("orca/other/feat"), &e),
        (Tool::Unknown, Ev::None)
    );
}

#[test]
fn a_bare_repo_has_no_main_root_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    // A bare repo: its common directory is not called `.git`.
    let common = base.join("proj.git");
    std::fs::create_dir_all(common.join("worktrees/wt-a")).unwrap();
    for root in [
        base.join("proj.feat"),
        base.join("proj/.claude/worktrees/x"),
        base.join("proj.git/.claude/worktrees/x"),
    ] {
        assert_eq!(
            detect(&common, &root, Some("wt-a"), false, &no_env()),
            (Tool::Unknown, Ev::None),
            "{}: no main root, no worktrunk and no claude-code rule",
            root.display()
        );
    }
    // The rules that do not depend on the main root still apply.
    let home = base.join("home");
    assert_eq!(
        detect(
            &common,
            &home.join("orca/workspaces/r/x"),
            Some("wt-a"),
            false,
            &env(&[&home], None)
        ),
        (Tool::Orca, Ev::PathPattern)
    );
}

#[test]
fn nothing_is_ever_gitkraken() {
    let l = land();
    let home = l.base.join("home");
    let e = env(&[&home], Some(&l.base.join("c")));
    for root in [
        l.base.join("proj.feat"),
        l.base.join("GitKraken/worktrees/x"),
        home.join(".gitkraken/worktrees/x"),
        l.main_root.join(".claude/worktrees/x"),
        l.base.join("manual"),
    ] {
        let (tool, _) = found(&l, &root, &e);
        assert_ne!(tool, Tool::Gitkraken, "{}", root.display());
    }
}

#[test]
fn a_vanished_root_does_not_panic() {
    let l = land();
    let gone = l.base.join("proj.gone");
    assert!(!gone.exists());
    // Still classified by its path; never an error.
    assert_eq!(
        found(&l, &gone, &no_env()),
        (Tool::Worktrunk, Ev::PathPattern)
    );
    let view = origin_of(
        &l.common,
        &gone,
        false,
        Some("wt-a"),
        false,
        &missing(),
        &no_env(),
    )
    .expect("a linked worktree carries an origin even when its folder is gone");
    assert_eq!(view.hands_off, Some(HandsOffReasonView::Unreadable));
    // The administration directory itself gone.
    std::fs::remove_dir_all(l.common.join("worktrees")).unwrap();
    assert_eq!(
        found(&l, &l.base.join("manual"), &no_env()),
        (Tool::Unknown, Ev::None)
    );
    assert!(!is_locked(&l.common, Some("wt-a")));
}

#[test]
fn locked_is_a_file_in_the_administration_directory() {
    let l = land();
    assert!(!is_locked(&l.common, Some("wt-a")));
    std::fs::write(l.common.join("worktrees/wt-a/locked"), "").unwrap();
    assert!(is_locked(&l.common, Some("wt-a")));
    assert!(!is_locked(&l.common, None));
    assert!(!is_locked(&l.common, Some("../wt-a")), "one component only");
}

#[test]
fn hands_off_reasons_and_their_precedence() {
    let det = ready(HeadView::Detached);
    assert_eq!(
        hands_off(Tool::Unknown, &det, false),
        Some(HandsOffReasonView::Detached)
    );
    assert_eq!(
        hands_off(Tool::Codex, &det, true),
        Some(HandsOffReasonView::Detached),
        "detached wins over locked"
    );
    assert_eq!(
        hands_off(Tool::Orca, &ready(branch()), true),
        Some(HandsOffReasonView::Locked)
    );
    assert_eq!(
        hands_off(Tool::Unknown, &missing(), false),
        Some(HandsOffReasonView::Unreadable)
    );
    assert_eq!(
        hands_off(Tool::Unknown, &missing(), true),
        Some(HandsOffReasonView::Locked),
        "locked wins over unreadable"
    );
    assert_eq!(hands_off(Tool::Orca, &ready(branch()), false), None);
    // GitRaptor's own worktrees are never hands off.
    for status in [det, missing(), ready(branch())] {
        assert_eq!(hands_off(Tool::Gitraptor, &status, true), None);
    }
}

#[test]
fn origin_of_the_main_worktree_is_none_and_a_linked_one_carries_it() {
    let l = land();
    assert!(
        origin_of(
            &l.common,
            &l.main_root,
            true,
            None,
            false,
            &ready(branch()),
            &no_env()
        )
        .is_none(),
        "the main worktree carries no origin"
    );
    let v = origin_of(
        &l.common,
        &l.base.join("proj.feat"),
        false,
        Some("wt-a"),
        false,
        &ready(HeadView::Detached),
        &no_env(),
    )
    .expect("a linked worktree carries an origin");
    assert_eq!(
        (v.tool, v.evidence, v.hands_off),
        (
            Tool::Worktrunk,
            Ev::PathPattern,
            Some(HandsOffReasonView::Detached)
        )
    );
}

fn tree(dir: &Path, out: &mut BTreeSet<(PathBuf, Vec<u8>)>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            tree(&p, out);
        } else {
            out.insert((p.clone(), std::fs::read(&p).unwrap()));
        }
    }
}

#[test]
fn detection_writes_nothing() {
    let l = land();
    std::fs::create_dir(l.common.join("worktrees/wt-a/orca")).unwrap();
    std::fs::write(l.common.join("worktrees/wt-a/locked"), "x").unwrap();
    let (mut before, mut after) = (BTreeSet::new(), BTreeSet::new());
    tree(&l.base, &mut before);
    assert_eq!(
        found(&l, &l.base.join("proj.feat"), &no_env()),
        (Tool::Orca, Ev::AdminMarker),
        "the detection must work before it can be judged read-only"
    );
    let _ = origin_of(
        &l.common,
        &l.base.join("proj.feat"),
        false,
        Some("wt-a"),
        false,
        &ready(branch()),
        &no_env(),
    );
    let _ = is_locked(&l.common, Some("wt-a"));
    tree(&l.base, &mut after);
    assert_eq!(before, after);
}

fn view(main: bool, origin: Option<serde_json::Value>) -> WorktreeView {
    let mut v = json!({
        "path": {"untrusted": "/w/feat"},
        "main": main,
        "admin_name": null,
        "status": {"state": "unavailable", "reason": "missing"}
    });
    if let Some(origin) = origin {
        v["origin"] = origin;
    }
    serde_json::from_value(v).expect("WorktreeView must accept origin")
}

#[test]
fn refuses_destructive_is_the_hook_of_future_operations() {
    // A linked worktree with no origin: nothing proves it is safe.
    assert!(refuses_destructive(&view(false, None)));
    // Marked hands off.
    for reason in ["detached", "locked", "unreadable"] {
        assert!(
            refuses_destructive(&view(
                false,
                Some(json!({"tool": "unknown", "evidence": "none", "hands_off": reason}))
            )),
            "hands_off {reason}"
        );
    }
    // The main worktree.
    assert!(!refuses_destructive(&view(true, None)));
    // A foreign one without the mark.
    assert!(!refuses_destructive(&view(
        false,
        Some(json!({"tool": "unknown", "evidence": "none"}))
    )));
    assert!(!refuses_destructive(&view(
        false,
        Some(json!({"tool": "orca", "evidence": "admin-marker"}))
    )));
}
