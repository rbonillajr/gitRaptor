//! SEC-05 (credentials never leave the layer), SEC-11 (`safe.directory`) and M1 (gitoxide
//! never launches `git`).

mod common;

use std::sync::Arc;

use common::busy_repo;
use gitraptor_git::cli::{ConfigKey, GitCli, RefNamespace};
use gitraptor_git::{MemoryArgvLog, ReadError, ReaderOptions, RefName, RepoReader};

const TOKEN: &str = "s3cr3t-T0KEN";

#[test]
fn remote_url_never_exposes_userinfo() {
    let f = busy_repo();
    f.git(&[
        "remote",
        "set-url",
        "origin",
        &format!("https://agent:{TOKEN}@example.com/o/r.git"),
    ]);
    let origin = RefName::new("origin").unwrap();
    let r = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
    assert_eq!(
        r.remote_url(&origin).unwrap().as_deref(),
        Some("https://example.com/o/r.git")
    );
    let invoker = f.invoker();
    let cli = GitCli::new(&f.git, &invoker, &f.repo).unwrap();
    assert_eq!(
        cli.config_get(&ConfigKey::RemoteUrl(origin))
            .unwrap()
            .as_deref(),
        Some("https://example.com/o/r.git")
    );
}

#[test]
fn extra_header_not_reachable() {
    let f = busy_repo();
    f.git(&[
        "remote",
        "set-url",
        "origin",
        &format!("https://agent:{TOKEN}@example.com/o/r.git"),
    ]);
    f.git(&[
        "config",
        "http.extraHeader",
        &format!("Authorization: Bearer {TOKEN}"),
    ]);
    // `ConfigKey` has no variant for `http.*`: the key is not representable. Everything the
    // layer returns or logs is checked for the token.
    let log = MemoryArgvLog::default();
    let invoker = f.invoker().with_argv_sink(Arc::new(log.clone()));
    let origin = RefName::new("origin").unwrap();
    let main = RefName::new("main").unwrap();
    let r = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
    let cli = GitCli::new(&f.git, &invoker, &f.repo).unwrap();
    let mut seen = vec![
        format!("{r:?}"),
        format!("{:?}", r.head()),
        format!("{:?}", r.local_branches()),
        format!("{:?}", r.status()),
        format!("{:?}", r.worktrees()),
        format!("{:?}", r.remote_url(&origin)),
        format!("{:?}", cli.for_each_ref(RefNamespace::Remotes)),
        format!("{:?}", cli.log(&main, 10)),
        format!(
            "{:?}",
            cli.config_get(&ConfigKey::RemoteUrl(origin.clone()))
        ),
        format!(
            "{:?}",
            cli.config_get(&ConfigKey::BranchRemote(main.clone()))
        ),
        format!("{invoker:?}"),
    ];
    seen.extend(log.entries().into_iter().map(|a| a.join(" ")));
    for s in seen {
        assert!(!s.contains(TOKEN), "token leaked: {s}");
        assert!(!s.contains("Authorization"), "header leaked: {s}");
    }
}

#[test]
fn untrusted_repo_is_unavailable() {
    let f = busy_repo();
    let before = f.fingerprint();
    let err = RepoReader::open(
        &f.repo,
        &ReaderOptions {
            force_reduced_trust: true,
            ignore_ambient_config: true,
        },
    )
    .unwrap_err();
    assert!(matches!(err, ReadError::Untrusted(_)), "{err:?}");
    common::assert_unchanged(&before, &f.fingerprint(), "untrusted open");
}

#[test]
fn reader_rejects_unc_and_relative_paths() {
    for p in ["//server/share/repo", "\\\\server\\share", "relative/repo"] {
        assert!(matches!(
            RepoReader::open(std::path::Path::new(p), &ReaderOptions::default()),
            Err(ReadError::InvalidInput(_))
        ));
    }
}

/// M1: gitoxide must not launch `git` (or a shell) by itself. The check runs in a child test
/// process whose `PATH` only holds fakes that leave a marker, because gix-path caches its
/// lookups per process.
#[cfg(unix)]
#[test]
fn gitoxide_never_launches_git() {
    let f = busy_repo();
    let bin = f.root().join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    let marker = f.root().join("git-launched");
    for name in ["git", "sh", "bash"] {
        common::script(
            &bin.join(name),
            // A shell redirection, not `touch`: `PATH` only holds the fakes.
            &format!(": > '{}'\nexit 1", marker.display()),
        );
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "m1_child_reads", "--ignored", "--nocapture"])
        .env("PATH", &bin)
        .env("M1_REPO", &f.repo)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("1 passed"), "{stdout}");
    assert!(!marker.exists(), "gitoxide launched a program from PATH");
}

/// Child half of [`gitoxide_never_launches_git`]; does nothing when run on its own.
#[test]
#[ignore = "run by gitoxide_never_launches_git"]
fn m1_child_reads() {
    let Some(repo) = std::env::var_os("M1_REPO") else {
        return;
    };
    let repo = std::path::PathBuf::from(repo);
    let r = RepoReader::open(&repo, &ReaderOptions::default()).unwrap();
    let main = RefName::new("main").unwrap();
    let feature = RefName::new("feature").unwrap();
    r.head().unwrap();
    r.local_branches().unwrap();
    r.index_entry_count().unwrap();
    r.status().unwrap();
    r.worktrees().unwrap();
    r.in_progress();
    r.is_ignored("target", true).unwrap();
    r.remote_url(&RefName::new("origin").unwrap()).unwrap();
    r.merge_base(&main, &feature).unwrap();
    r.ahead_behind(&main, &feature, 100).unwrap();
}
