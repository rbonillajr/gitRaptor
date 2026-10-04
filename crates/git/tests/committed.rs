//! Reads of committed objects (TS-GRD-001): file at a path, blob by id, remotes and symbolic
//! refs, with replacement objects and grafts ignored (SEC-GRD-17).

mod common;

use common::Fixture;
use gitraptor_git::{BlobRead, CommittedFile, NotRegular, ReaderOptions, RefName, RepoReader};

const PATH: &[&str] = &[".gitraptor", "settings.json"];

fn reader(f: &Fixture) -> RepoReader {
    RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap()
}

fn head(f: &Fixture) -> String {
    f.git(&["rev-parse", "HEAD"]).trim().to_owned()
}

fn commit_settings(f: &Fixture, content: &str) -> String {
    f.write(".gitraptor/settings.json", content);
    f.git(&["add", "."]);
    f.git(&["commit", "-q", "-m", "settings"]);
    head(f)
}

#[test]
fn reads_the_committed_blob_not_the_working_tree() {
    let f = Fixture::with_commit();
    let commit = commit_settings(&f, "{\"a\":1}");
    f.write(".gitraptor/settings.json", "{\"a\":2}");
    let r = reader(&f);
    match r.committed_file(&commit, PATH, 1024).unwrap() {
        CommittedFile::Blob { id, bytes } => {
            assert_eq!(bytes, b"{\"a\":1}");
            let expected = f.git(&["rev-parse", &format!("{commit}:.gitraptor/settings.json")]);
            assert_eq!(id, expected.trim());
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn absent_path_and_absent_directory() {
    let f = Fixture::with_commit();
    let r = reader(&f);
    assert_eq!(
        r.committed_file(&head(&f), PATH, 1024).unwrap(),
        CommittedFile::Absent
    );
    let commit = commit_settings(&f, "{}");
    assert_eq!(
        r.committed_file(&commit, &[".gitraptor", "other.json"], 1024)
            .unwrap(),
        CommittedFile::Absent
    );
}

/// Write `content` as a blob and return its id.
fn hash_blob(f: &Fixture, content: &str) -> String {
    use std::io::Write;
    let mut child = f
        .git_command(&f.repo, &["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Commit an index entry with `mode` and `id` at the settings path, on any OS.
fn commit_entry(f: &Fixture, mode: &str, id: &str) -> String {
    let _ = f
        .git_command(
            &f.repo,
            &["rm", "-q", "--cached", ".gitraptor/settings.json"],
        )
        .output();
    f.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("{mode},{id},.gitraptor/settings.json"),
    ]);
    f.git(&["commit", "-q", "-m", mode]);
    head(f)
}

#[test]
fn symlink_and_submodule_are_not_regular() {
    let f = Fixture::with_commit();
    let r = reader(&f);
    let target = hash_blob(&f, "/etc/hosts");
    let link = commit_entry(&f, "120000", &target);
    assert_eq!(
        r.committed_file(&link, PATH, 1024).unwrap(),
        CommittedFile::NotRegular(NotRegular::Symlink)
    );
    let submodule = commit_entry(&f, "160000", &link);
    assert_eq!(
        r.committed_file(&submodule, PATH, 1024).unwrap(),
        CommittedFile::NotRegular(NotRegular::Submodule)
    );
    // An executable file is still a regular blob.
    let blob = hash_blob(&f, "{}");
    let exec = commit_entry(&f, "100755", &blob);
    assert!(matches!(
        r.committed_file(&exec, PATH, 1024).unwrap(),
        CommittedFile::Blob { .. }
    ));
}

#[test]
fn directory_or_file_of_the_wrong_kind() {
    let f = Fixture::with_commit();
    f.write(".gitraptor/settings.json/inner", "x");
    f.git(&["add", "."]);
    f.git(&["commit", "-q", "-m", "dir"]);
    let r = reader(&f);
    assert_eq!(
        r.committed_file(&head(&f), PATH, 1024).unwrap(),
        CommittedFile::NotRegular(NotRegular::WrongKind)
    );
    let f = Fixture::with_commit();
    f.write(".gitraptor", "a file");
    f.git(&["add", "."]);
    f.git(&["commit", "-q", "-m", "file"]);
    assert_eq!(
        reader(&f).committed_file(&head(&f), PATH, 1024).unwrap(),
        CommittedFile::NotRegular(NotRegular::WrongKind)
    );
}

#[test]
fn oversized_blob_is_not_loaded() {
    let f = Fixture::with_commit();
    let commit = commit_settings(&f, &"x".repeat(2048));
    assert_eq!(
        reader(&f).committed_file(&commit, PATH, 1024).unwrap(),
        CommittedFile::TooLarge { size: 2048 }
    );
}

#[test]
fn replacement_objects_do_not_change_the_read() {
    let f = Fixture::with_commit();
    let commit = commit_settings(&f, "{\"strict\":true}");
    let blob = f.git(&["rev-parse", &format!("{commit}:.gitraptor/settings.json")]);
    let lax = hash_blob(&f, "{\"strict\":false}");
    f.git(&["replace", blob.trim(), &lax]);
    // Git itself now sees the lax content…
    assert_eq!(
        f.git(&["cat-file", "-p", blob.trim()]),
        "{\"strict\":false}"
    );
    // …the reader does not, by path or by id.
    let r = reader(&f);
    match r.committed_file(&commit, PATH, 1024).unwrap() {
        CommittedFile::Blob { bytes, .. } => assert_eq!(bytes, b"{\"strict\":true}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(
        r.blob_by_id(blob.trim(), 1024).unwrap(),
        BlobRead::Blob {
            bytes: b"{\"strict\":true}".to_vec()
        }
    );

    // gix 0.88 reads `core.useReplaceRefs` inverted; neither value in the repo config may turn
    // replacements back on.
    for value in ["false", "true"] {
        f.git(&["config", "core.useReplaceRefs", value]);
        match reader(&f).committed_file(&commit, PATH, 1024).unwrap() {
            CommittedFile::Blob { bytes, .. } => assert_eq!(bytes, b"{\"strict\":true}", "{value}"),
            other => panic!("unexpected {other:?}"),
        }
    }
    f.git(&["config", "--unset", "core.useReplaceRefs"]);

    // Replacing the `.gitraptor` tree with one holding the lax blob changes nothing either.
    let lax_tree = {
        use std::io::Write;
        let mut child = f
            .git_command(&f.repo, &["mktree"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "100644 blob {lax}\tsettings.json"
        )
        .unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
    };
    f.git(&["replace", "--delete", blob.trim()]);
    let tree = f.git(&["rev-parse", &format!("{commit}:.gitraptor")]);
    f.git(&["replace", tree.trim(), lax_tree.trim()]);
    assert_eq!(
        f.git(&[
            "cat-file",
            "-p",
            &format!("{commit}:.gitraptor/settings.json")
        ]),
        "{\"strict\":false}"
    );
    match reader(&f).committed_file(&commit, PATH, 1024).unwrap() {
        CommittedFile::Blob { bytes, .. } => assert_eq!(bytes, b"{\"strict\":true}"),
        other => panic!("unexpected {other:?}"),
    }

    // Replacing the whole commit with one whose tree has no settings changes nothing either.
    f.git(&["checkout", "-q", "--orphan", "empty"]);
    f.git(&["rm", "-rq", "--cached", "."]);
    f.git(&["commit", "-q", "--allow-empty", "-m", "empty"]);
    let empty = head(&f);
    f.git(&["replace", &commit, &empty]);
    assert!(matches!(
        reader(&f).committed_file(&commit, PATH, 1024).unwrap(),
        CommittedFile::Blob { .. }
    ));
}

#[test]
fn grafts_do_not_change_the_read() {
    let f = Fixture::with_commit();
    let base = head(&f);
    let commit = commit_settings(&f, "{\"strict\":true}");
    std::fs::create_dir_all(f.repo.join(".git/info")).unwrap();
    std::fs::write(
        f.repo.join(".git/info/grafts"),
        format!("{commit} {base}\n"),
    )
    .unwrap();
    match reader(&f).committed_file(&commit, PATH, 1024).unwrap() {
        CommittedFile::Blob { bytes, .. } => assert_eq!(bytes, b"{\"strict\":true}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn blob_by_id_reports_missing_and_kind() {
    let f = Fixture::with_commit();
    let r = reader(&f);
    assert_eq!(
        r.blob_by_id(&"1".repeat(40), 1024).unwrap(),
        BlobRead::Missing
    );
    assert_eq!(r.blob_by_id(&head(&f), 1024).unwrap(), BlobRead::NotABlob);
    assert!(r.blob_by_id("not-hex", 1024).is_err());
}

#[test]
fn rejects_unsafe_paths() {
    let f = Fixture::with_commit();
    let r = reader(&f);
    for bad in [&[][..], &[".."], &["a/b"], &[""]] {
        assert!(r.committed_file(&head(&f), bad, 1024).is_err());
    }
}

#[test]
fn remotes_and_symbolic_targets() {
    let f = Fixture::with_commit();
    let r = reader(&f);
    assert!(r.remote_names().is_empty());
    f.git(&["remote", "add", "upstream", "/nonexistent/upstream"]);
    f.git(&["remote", "add", "origin", "/nonexistent/origin"]);
    let commit = head(&f);
    f.git(&["update-ref", "refs/remotes/origin/trunk", &commit]);
    f.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/trunk",
    ]);
    let r = reader(&f);
    let names: Vec<_> = r.remote_names().iter().map(|n| n.to_string()).collect();
    assert_eq!(names, ["origin", "upstream"]);
    let head_ref = RefName::new("refs/remotes/origin/HEAD").unwrap();
    assert_eq!(
        r.symbolic_target(&head_ref).unwrap().as_deref(),
        Some("refs/remotes/origin/trunk")
    );
    let direct = RefName::new("refs/remotes/origin/trunk").unwrap();
    assert_eq!(r.symbolic_target(&direct).unwrap(), None);
    let none = RefName::new("refs/remotes/upstream/HEAD").unwrap();
    assert_eq!(r.symbolic_target(&none).unwrap(), None);
}

#[test]
fn committed_reads_leave_the_repo_untouched() {
    let f = Fixture::with_commit();
    let commit = commit_settings(&f, "{}");
    let before = f.fingerprint();
    let r = reader(&f);
    let _ = r.committed_file(&commit, PATH, 1024).unwrap();
    let _ = r.remote_names();
    drop(r);
    common::assert_unchanged(&before, &f.fingerprint(), "committed reads");
}

/// The replacement variables of the engine process do not turn replacements on: the reader
/// isolates the environment. `set_var` is unsafe (and `unsafe` is forbidden), so the test runs
/// the replacement test again in a child process with the variables set.
#[test]
fn replacement_environment_variables_are_ignored() {
    let exe = std::env::current_exe().unwrap();
    for (key, value) in [
        ("GIT_NO_REPLACE_OBJECTS", "1"),
        ("GIT_NO_REPLACE_OBJECTS", "0"),
        ("GIT_NO_REPLACE_OBJECTS", ""),
        ("GIT_REPLACE_REF_BASE", "refs/replace/"),
    ] {
        let out = std::process::Command::new(&exe)
            .args([
                "--exact",
                "replacement_objects_do_not_change_the_read",
                "--test-threads=1",
            ])
            .env(key, value)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "{key}={value}: {stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
