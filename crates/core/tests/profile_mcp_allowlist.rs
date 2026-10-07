//! The MCP allowlist as a mark of the observed repo (US-MCP-002, ADR-GRP-006
//! Enmienda (2026-10-05, MCP)): a subset of the observed repos by
//! construction, cleared with the retirement and not restored by a re-add.

mod common;

use common::*;
use gitraptor_core::profile::INDEX_FILE;

#[test]
fn only_an_observed_repo_can_be_enabled() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let main = init_repo(repos.path(), "shop", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&main), None, 1).unwrap();
    let id = entry.repo_id;

    // Observing does not enable.
    assert!(profile.mcp_enabled_repos().unwrap().is_empty());
    assert_eq!(
        profile.set_mcp_enabled(&id, true, "cli", 2).unwrap(),
        Some(true)
    );
    // Idempotent.
    assert_eq!(
        profile.set_mcp_enabled(&id, true, "cli", 3).unwrap(),
        Some(false)
    );
    assert_eq!(
        profile.mcp_enabled_repos().unwrap(),
        std::slice::from_ref(&id)
    );
    // An unknown repo is not observed: nothing changes.
    assert_eq!(
        profile
            .set_mcp_enabled("0a1b2c3d-0000-4000-8000-0000000000ff", true, "cli", 4)
            .unwrap(),
        None
    );
    // Disabling keeps the repo observed.
    assert_eq!(
        profile.set_mcp_enabled(&id, false, "cli", 5).unwrap(),
        Some(true)
    );
    assert!(profile.mcp_enabled_repos().unwrap().is_empty());
    assert_eq!(profile.repos().unwrap().len(), 1);
}

#[test]
fn retiring_clears_the_mark_and_re_adding_does_not_restore_it() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let main = init_repo(repos.path(), "shop", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&main), None, 1).unwrap();
    let id = entry.repo_id;
    profile.set_mcp_enabled(&id, true, "cli", 2).unwrap();

    profile.retire_repo(&id, 3).unwrap();
    assert!(profile.mcp_enabled_repos().unwrap().is_empty());
    // A retired repo cannot be enabled.
    assert_eq!(profile.set_mcp_enabled(&id, true, "cli", 4).unwrap(), None);

    profile.add_repo(&common_dir(&main), None, 5).unwrap();
    assert!(profile.mcp_enabled_repos().unwrap().is_empty());
}

/// A profile written before the migration opens, keeps its repos and has
/// none of them enabled.
#[test]
fn a_profile_from_before_the_mark_migrates_and_keeps_its_repos() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let main = init_repo(repos.path(), "shop", true);
    let id = {
        let mut profile = tp.open();
        profile
            .add_repo(&common_dir(&main), None, 1)
            .unwrap()
            .0
            .repo_id
    };
    // Back to the previous schema (user_version 2, no mark columns).
    {
        let conn = rusqlite::Connection::open(tp.dirs().data.join(INDEX_FILE)).unwrap();
        conn.execute_batch(
            "ALTER TABLE repos DROP COLUMN mcp_enabled_ms;
             ALTER TABLE repos DROP COLUMN mcp_enabled_by;
             PRAGMA user_version = 2;",
        )
        .unwrap();
    }
    let mut profile = tp.open();
    let all = profile.repos().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].repo_id, id);
    assert!(profile.mcp_enabled_repos().unwrap().is_empty());
    assert_eq!(
        profile.set_mcp_enabled(&id, true, "cli", 2).unwrap(),
        Some(true)
    );
    drop(profile);
    // Opening again migrates nothing and keeps the mark.
    let profile = tp.open();
    assert_eq!(profile.mcp_enabled_repos().unwrap(), [id]);
}
