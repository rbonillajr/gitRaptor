//! US-WTL-005 (T004): `reconcile` publishes who created each linked worktree
//! (`origin`) and the "foreign, hands off" mark, without writing anything in
//! any worktree or in the administration directory (BR-WTL-011, NFR-01).
//! Temporary repos of the testkit; the field is read from the JSON on
//! purpose, so this file builds before the `origin` field exists.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gitraptor_api::messages::WorktreeView;
use gitraptor_core::observe::{base_branch, locate, reconcile};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

struct Shop {
    f: Fixture,
    /// `<root>/repo.feat`: worktrunk's default template, a sibling of the main root.
    trunk: PathBuf,
    /// `<root>/wt-orca` with a real `orca/` folder in its administration directory.
    orca: PathBuf,
    /// `<root>/manual`: made by hand, on a branch, in another folder.
    manual: PathBuf,
}

fn shop() -> Shop {
    let f = Fixture::with_commit(&git_from_path());
    let trunk = f.root.join("repo.feat");
    let orca = f.root.join("wt-orca");
    let manual = f.root.join("manual");
    for (path, branch) in [(&trunk, "feat"), (&orca, "orca-b"), (&manual, "manual-b")] {
        f.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            path.to_str().unwrap(),
        ]);
    }
    std::fs::create_dir_all(f.repo.join(".git/worktrees/wt-orca/orca")).unwrap();
    std::fs::write(
        f.repo.join(".git/worktrees/wt-orca/orca/setup-runner.sh"),
        "#!/bin/sh\n",
    )
    .unwrap();
    // Config files of other tools, which must stay as they are.
    std::fs::write(f.repo.join(".worktreeinclude"), ".env\n").unwrap();
    std::fs::create_dir_all(f.repo.join(".config")).unwrap();
    std::fs::write(f.repo.join(".config/wt.toml"), "[post-create]\n").unwrap();
    Shop {
        f,
        trunk,
        orca,
        manual,
    }
}

fn read(s: &Shop) -> Vec<WorktreeView> {
    let common = locate(&s.f.repo).unwrap();
    reconcile(&common, &base_branch(None)).unwrap().views()
}

fn json_of(view: &WorktreeView) -> Value {
    serde_json::to_value(view).unwrap()
}

fn find<'a>(views: &'a [Value], path: &Path) -> &'a Value {
    let canonical = gitraptor_core::observe::canonical(path);
    views
        .iter()
        .find(|v| v["path"]["untrusted"] == canonical.to_str().unwrap())
        .unwrap_or_else(|| panic!("no worktree {} in {views:?}", path.display()))
}

fn all_json(s: &Shop) -> Vec<Value> {
    read(s).iter().map(json_of).collect()
}

#[test]
fn fleet_shows_origin_when_known() {
    let s = shop();
    let views = all_json(&s);
    assert_eq!(views.len(), 4, "{views:?}");

    let trunk = find(&views, &s.trunk);
    assert_eq!(
        trunk["origin"],
        json!({"tool": "worktrunk", "evidence": "path-pattern"}),
        "a sibling `repo.feat` must read as worktrunk by path (O3, scenario 1): {trunk}"
    );
    let orca = find(&views, &s.orca);
    assert_eq!(
        orca["origin"],
        json!({"tool": "orca", "evidence": "admin-marker"}),
        "the `orca/` mark in the administration directory must read as orca (O3): {orca}"
    );
    let manual = find(&views, &s.manual);
    assert_eq!(
        manual["origin"],
        json!({"tool": "unknown", "evidence": "none"}),
        "a worktree made by hand must read as foreign, unknown/none (O3): {manual}"
    );
    let main = views.iter().find(|v| v["main"] == true).unwrap();
    assert!(
        main.get("origin").is_none(),
        "the main worktree carries no origin: {main}"
    );
}

#[test]
fn detached_foreign_is_hands_off() {
    let s = shop();
    let detached = s.f.root.join("hand-detached");
    s.f.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        detached.to_str().unwrap(),
    ]);
    let views = all_json(&s);
    assert_eq!(
        find(&views, &detached)["origin"],
        json!({"tool": "unknown", "evidence": "none", "hands_off": "detached"}),
        "a foreign worktree with a detached HEAD must be marked hands_off detached (O3)"
    );
    // The ones on a branch are not marked.
    assert!(
        find(&views, &s.manual)["origin"].get("hands_off").is_none(),
        "a foreign worktree on a branch must not be hands_off"
    );
}

#[test]
fn locked_foreign_is_hands_off() {
    let s = shop();
    s.f.git(&["worktree", "lock", s.manual.to_str().unwrap()]);
    let views = all_json(&s);
    assert_eq!(
        find(&views, &s.manual)["origin"],
        json!({"tool": "unknown", "evidence": "none", "hands_off": "locked"}),
        "a locked foreign worktree must be marked hands_off locked (O3)"
    );
}

#[test]
fn unreadable_foreign_is_hands_off() {
    let s = shop();
    std::fs::remove_dir_all(&s.manual).unwrap();
    let views = all_json(&s);
    let manual = find(&views, &s.manual);
    assert_eq!(manual["status"]["state"], "unavailable", "{manual}");
    assert_eq!(
        manual["origin"],
        json!({"tool": "unknown", "evidence": "none", "hands_off": "unreadable"}),
        "an unavailable foreign worktree must still carry origin, hands_off unreadable (O3)"
    );
}

/// Content and mtime of every file under `dir`.
fn tree(dir: &Path, out: &mut BTreeMap<PathBuf, (Vec<u8>, SystemTime)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).unwrap();
        if meta.is_dir() {
            tree(&path, out);
        } else {
            let bytes = std::fs::read(&path).unwrap_or_default();
            out.insert(path, (bytes, meta.modified().unwrap()));
        }
    }
}

fn watched(s: &Shop) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
    let mut out = BTreeMap::new();
    tree(&s.f.repo.join(".git/worktrees"), &mut out);
    for rela in [".worktreeinclude", ".config/wt.toml", ".git/config"] {
        let p = s.f.repo.join(rela);
        out.insert(
            p.clone(),
            (
                std::fs::read(&p).unwrap(),
                std::fs::metadata(&p).unwrap().modified().unwrap(),
            ),
        );
    }
    out
}

#[test]
fn other_tool_config_is_untouched() {
    let s = shop();
    // The feature must be there: otherwise "untouched" proves nothing.
    let first = all_json(&s);
    assert!(
        find(&first, &s.orca).get("origin").is_some(),
        "the engine must publish origin before this test can judge that it writes nothing"
    );
    let before = watched(&s);
    let _ = read(&s);
    let _ = read(&s);
    assert_eq!(
        before,
        watched(&s),
        "two reconciliations must leave .worktreeinclude, .config/wt.toml, .git/config and the administration directory identical (BR-WTL-011)"
    );
}

#[test]
fn vanished_worktree_leaves_without_error() {
    let s = shop();
    let before = all_json(&s);
    assert!(
        find(&before, &s.orca).get("origin").is_some(),
        "the engine must publish origin (O3)"
    );
    s.f.git(&["worktree", "remove", "--force", s.trunk.to_str().unwrap()]);
    let after = all_json(&s);
    assert_eq!(
        after.len(),
        before.len() - 1,
        "a foreign worktree removed with git must stop showing: {after:?}"
    );
    let canonical = gitraptor_core::observe::canonical(&s.trunk);
    assert!(
        after
            .iter()
            .all(|v| v["path"]["untrusted"] != canonical.to_str().unwrap()),
        "the removed worktree is still listed"
    );
    for kept in [&s.orca, &s.manual] {
        assert_eq!(
            find(&before, kept),
            find(&after, kept),
            "the other worktrees must not change when one disappears"
        );
    }
}
