//! Contract tests of TD-GRP-007 (written by the criterion-tester): the exact argv templates of
//! the daemon's inert `git`s (GRP007-ARGV, GRP007-FIT).

use super::*;

fn os(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

fn root() -> PathBuf {
    PathBuf::from(if cfg!(windows) { "C:\\r" } else { "/r" })
}

/// The real write-layer argv of `subcommand`, with or without `--work-tree`.
fn write(subcommand: WriteSubcommand, work_tree: bool) -> Vec<OsString> {
    let r = root();
    let git_dir = r.join(".git");
    let hooks = r.join("hooks-empty");
    let global = r.join("global-empty");
    let index = r.join("index-tmp");
    let target = WriteTarget {
        git_dir: &git_dir,
        work_tree: work_tree.then_some(r.as_path()),
        hooks_dir: &hooks,
        global_config: &global,
        index_file: Some(&index),
    };
    write_argv(&target, subcommand)
}

/// The real read-layer argv of `subcommand` followed by `extra`.
fn read(subcommand: Subcommand, extra: &[&str]) -> Vec<OsString> {
    let mut argv = os(FIXED_OPTIONS);
    argv.extend(subcommand.words().iter().map(OsString::from));
    argv.extend(extra.iter().map(OsString::from));
    argv
}

/// The unmutated argv must be inert, so a negative case proves the mutation and not a template
/// that never matches.
fn assert_base_inert(argv: &[OsString]) {
    assert!(
        is_daemon_inert_argv(argv),
        "GRP007-ARGV: the unmutated argv must be inert before the mutation: {argv:?}"
    );
}

/// Index of the `--git-dir=` word of a write argv.
fn git_dir_at(argv: &[OsString]) -> usize {
    argv.iter()
        .position(|a| a.to_string_lossy().starts_with("--git-dir="))
        .expect("a write argv carries --git-dir")
}

#[test]
fn argv_write_templates_other_than_update_ref_are_inert() {
    for sub in [
        WriteSubcommand::IndexInfo,
        WriteSubcommand::SkipWorktree,
        WriteSubcommand::PackObjects,
        WriteSubcommand::IndexPack,
        WriteSubcommand::Repack,
        WriteSubcommand::Prune,
    ] {
        for work_tree in [true, false] {
            let argv = write(sub, work_tree);
            assert!(
                is_daemon_inert_argv(&argv),
                "GRP007-ARGV: the real write argv of {:?} (work-tree: {work_tree}) must be inert: {argv:?}",
                sub.words()
            );
        }
    }
}

#[test]
fn argv_read_templates_with_arguments_are_inert() {
    let cases: [(Subcommand, &[&str]); 6] = [
        (Subcommand::Version, &["--build-options"]),
        (Subcommand::RevParse, &["--verify", "HEAD"]),
        (
            Subcommand::ForEachRef,
            &["--format=%(refname)", "refs/heads"],
        ),
        (Subcommand::RevList, &["--max-count=5", "HEAD"]),
        (Subcommand::MergeBase, &["main", "feature"]),
        (Subcommand::Log, &["-1", "--format=%H", "HEAD"]),
    ];
    for (sub, extra) in cases {
        let argv = read(sub, extra);
        assert!(
            is_daemon_inert_argv(&argv),
            "GRP007-ARGV: the real read argv of {:?} with arguments must be inert: {argv:?}",
            sub.words()
        );
    }
}

#[test]
fn argv_update_ref_is_not_inert() {
    for work_tree in [true, false] {
        let argv = write(WriteSubcommand::UpdateRef, work_tree);
        assert!(
            !is_daemon_inert_argv(&argv),
            "GRP007-ARGV: update-ref moves refs and must never be inert (work-tree: {work_tree})"
        );
    }
}

#[test]
fn argv_read_worktree_list_config_and_exec_path_are_not_inert() {
    for (sub, label) in [
        (Subcommand::WorktreeList, "worktree list"),
        (Subcommand::Config, "config"),
        (Subcommand::ExecPath, "--exec-path"),
    ] {
        let argv = read(sub, &[]);
        assert!(
            !is_daemon_inert_argv(&argv),
            "GRP007-ARGV: {label} must not be inert: {argv:?}"
        );
    }
}

#[test]
fn argv_an_extra_dash_c_is_not_inert() {
    let mut write_argv_ = write(WriteSubcommand::PackObjects, true);
    assert_base_inert(&write_argv_);
    let at = git_dir_at(&write_argv_);
    write_argv_.splice(at..at, os(&["-c", "core.editor=evil"]));
    assert!(
        !is_daemon_inert_argv(&write_argv_),
        "GRP007-ARGV: an extra -c in the write template must not be inert"
    );

    let mut read_argv = os(FIXED_OPTIONS);
    read_argv.extend(os(&["log", "-1"]));
    assert_base_inert(&read_argv);
    read_argv.truncate(FIXED_OPTIONS.len());
    read_argv.extend(os(&["-c", "core.editor=evil"]));
    read_argv.extend(os(&["log", "-1"]));
    assert!(
        !is_daemon_inert_argv(&read_argv),
        "GRP007-ARGV: an extra -c in the read template must not be inert"
    );
}

#[test]
fn argv_a_missing_write_option_is_not_inert() {
    let argv = write(WriteSubcommand::PackObjects, true);
    assert_base_inert(&argv);
    // Drop the `-c gc.auto=0` pair.
    let at = argv
        .iter()
        .position(|a| a == "gc.auto=0")
        .expect("gc.auto=0 is a write option");
    let mut missing = argv.clone();
    missing.drain(at - 1..=at);
    assert!(
        !is_daemon_inert_argv(&missing),
        "GRP007-ARGV: a write argv missing one WRITE_OPTIONS pair must not be inert"
    );
}

#[test]
fn argv_a_changed_write_option_is_not_inert() {
    let mut argv = write(WriteSubcommand::PackObjects, true);
    assert_base_inert(&argv);
    let at = argv
        .iter()
        .position(|a| a == "commit.gpgSign=false")
        .expect("commit.gpgSign=false is a write option");
    argv[at] = OsString::from("commit.gpgSign=true");
    assert!(
        !is_daemon_inert_argv(&argv),
        "GRP007-ARGV: a write option with another value must not be inert"
    );
}

#[test]
fn argv_reordered_write_options_are_not_inert() {
    let mut argv = write(WriteSubcommand::PackObjects, true);
    assert_base_inert(&argv);
    let a = argv
        .iter()
        .position(|w| w == "core.fsmonitor=false")
        .expect("core.fsmonitor=false is a write option");
    let b = argv
        .iter()
        .position(|w| w == "core.splitIndex=false")
        .expect("core.splitIndex=false is a write option");
    argv.swap(a, b);
    assert!(
        !is_daemon_inert_argv(&argv),
        "GRP007-ARGV: write options in another order must not be inert"
    );
}

#[test]
fn argv_a_relative_git_dir_is_not_inert() {
    let mut argv = write(WriteSubcommand::PackObjects, false);
    assert_base_inert(&argv);
    let at = git_dir_at(&argv);
    argv[at] = OsString::from("--git-dir=.git");
    assert!(
        !is_daemon_inert_argv(&argv),
        "GRP007-ARGV: a relative --git-dir must not be inert"
    );
}

#[test]
fn argv_an_argument_after_the_write_template_is_not_inert() {
    for sub in [WriteSubcommand::PackObjects, WriteSubcommand::Prune] {
        let mut argv = write(sub, true);
        assert_base_inert(&argv);
        argv.push(OsString::from("extra"));
        assert!(
            !is_daemon_inert_argv(&argv),
            "GRP007-ARGV: an argument after the {:?} template must not be inert",
            sub.words()
        );
    }
}

#[test]
fn argv_output_option_in_a_read_is_not_inert() {
    for extra in [&["--output", "x"][..], &["--output=x"][..]] {
        for sub in [Subcommand::Log, Subcommand::RevList] {
            assert_base_inert(&read(sub, &["HEAD"]));
            let argv = read(sub, extra);
            assert!(
                !is_daemon_inert_argv(&argv),
                "GRP007-ARGV: {:?} with {extra:?} writes a file and must not be inert",
                sub.words()
            );
        }
    }
}

#[test]
fn argv_without_fixed_options_or_empty_is_not_inert() {
    assert_base_inert(&write(WriteSubcommand::IndexInfo, true));
    assert!(
        !is_daemon_inert_argv(&os(&["update-index"])),
        "GRP007-ARGV: a bare `git update-index` has no fixed options"
    );
    assert!(
        !is_daemon_inert_argv(&os(&["update-index", "-z", "--index-info"])),
        "GRP007-ARGV: the update-index words without WRITE_OPTIONS must not be inert"
    );
    assert!(
        !is_daemon_inert_argv(&os(&["log", "-1"])),
        "GRP007-ARGV: a bare `git log` has no fixed options"
    );
    assert!(
        !is_daemon_inert_argv(&[]),
        "GRP007-ARGV: an empty argv must not be inert"
    );
}

#[test]
fn fitness_inert_iff_the_subcommand_cannot_move_refs_or_worktrees() {
    let argvs = daemon_git_argvs();
    assert!(
        !argvs.is_empty(),
        "daemon_git_argvs lists the daemon's git argvs"
    );
    for (first_word, argv) in argvs {
        let expected = !matches!(
            first_word,
            "update-ref" | "worktree" | "config" | "--exec-path"
        );
        assert_eq!(
            is_daemon_inert_argv(&argv),
            expected,
            "GRP007-FIT: `{first_word}` must be inert exactly when it cannot move a ref, HEAD or the worktree list: {argv:?}"
        );
    }
}
