//! US-MCP-010 T002: the lexical form of the paths of a `commit` (DS-010 § Contrato de API).
//! `Invalid` is a malformed path, `Outside` leaves the worktree or opens `.git`, `TooMany` is the
//! 257th path.

use gitraptor_api::catalog::MAX_COMMIT_PATHS;
use gitraptor_api::commit_paths::{
    PathRefusal, check_commit_path, check_commit_paths, is_dotgit_alias, is_git_special_name,
};

fn expect(path: &str, want: Result<(), PathRefusal>) {
    assert_eq!(
        check_commit_path(path),
        want,
        "{path:?} (escaped: {:?})",
        path.escape_unicode().to_string()
    );
}

#[test]
fn plain_relative_paths_pass() {
    for ok in [
        "a.rs",
        "src/a.rs",
        "a/b/c.txt",
        "dir with space/file name",
        "ñandú/日本語.rs",
        ".gitkeep",
        ".github/workflows/ci.yml",
        "a-b/-c-",
        "a.git",
        "git",
        "gitx/y",
        "x.gitignore.bak",
    ] {
        expect(ok, Ok(()));
    }
}

#[test]
fn absolute_paths_are_invalid() {
    for p in ["/etc/passwd", "/", "/a"] {
        expect(p, Err(PathRefusal::Invalid));
    }
}

#[test]
fn unc_drive_and_backslash_are_invalid() {
    for p in [
        "\\\\servidor\\compartido\\a.rs",
        "//servidor/compartido/a.rs",
        "C:x",
        "c:/x",
        "Z:\\x",
        "a\\b",
        "a/b\\c",
        "\\a",
    ] {
        expect(p, Err(PathRefusal::Invalid));
    }
}

#[test]
fn control_and_bidi_are_invalid() {
    for p in [
        "a\0b",
        "a\nb",
        "a\tb",
        "a\rb",
        "a\u{1b}b",
        "a\u{7f}b",
        "a\u{85}b",
        "a\u{202e}b",
        "a\u{2066}b",
        "a\u{200b}b",
        "a\u{feff}b",
        "a\u{2028}b",
    ] {
        expect(p, Err(PathRefusal::Invalid));
    }
}

#[test]
fn leading_dash_is_invalid() {
    for p in ["-f", "--amend", "-", "-rf/x"] {
        expect(p, Err(PathRefusal::Invalid));
    }
}

#[test]
fn not_canonical_paths_are_invalid() {
    for p in ["", "a//b", "a/", "./a", "a/./b", ".", "a/.", "//", "a//"] {
        expect(p, Err(PathRefusal::Invalid));
    }
    // over MAX_PATH_BYTES (4096), before looking at the characters
    expect(&"a".repeat(4097), Err(PathRefusal::Invalid));
    let mut long_with_bidi = "a".repeat(4097);
    long_with_bidi.push('\u{202e}');
    expect(&long_with_bidi, Err(PathRefusal::Invalid));
    expect(&"a".repeat(4096), Ok(()));
}

#[test]
fn dot_dot_is_outside() {
    for p in [
        "..",
        "../otro-repo/secreto",
        "a/../../x",
        "a/..",
        "a/../b",
        "x/y/../../..",
    ] {
        expect(p, Err(PathRefusal::Outside));
    }
}

#[test]
fn dot_git_is_outside() {
    for p in [".git", ".git/config", "sub/.git/hooks/x", "a/.git"] {
        expect(p, Err(PathRefusal::Outside));
    }
}

/// ⛔2.2: every alias of `.git` is `Outside`, on every platform.
#[test]
fn dot_git_aliases_are_outside_on_every_platform() {
    for p in [
        ".GIT",
        ".Git/config",
        "sub/.GIT/hooks/x",
        "GIT~1/config",
        "git~1",
        "Git~1/x",
        ".git./x",
        ".git /x",
        ".git../x",
        ".git. ./x",
        ".git::$INDEX_ALLOCATION",
        ".git:stream/x",
        // HFS+ ignorable code points that is_forbidden_char does not cover (CVE-2014-9390)
        ".g\u{206A}it/x",
        ".gi\u{206F}t/x",
        "\u{206B}.git/x",
        "a/.g\u{206C}\u{206D}\u{206E}it",
    ] {
        expect(p, Err(PathRefusal::Outside));
    }
    for alias in [
        ".git",
        ".GIT",
        "GIT~1",
        "git~1",
        ".git.",
        ".git ",
        ".git::$INDEX_ALLOCATION",
        ".g\u{200C}it",
        ".g\u{200D}it",
        ".g\u{202A}it",
        ".g\u{206A}it",
        ".g\u{206F}it",
        ".g\u{FEFF}it",
    ] {
        assert!(is_dotgit_alias(alias), "{alias:?} is an alias of .git");
    }
    for not in [
        ".gitignore",
        ".github",
        ".gitkeep",
        "git",
        ".gi",
        "git~2",
        "x.git",
        "",
    ] {
        assert!(!is_dotgit_alias(not), "{not:?} is not an alias of .git");
    }
    for special in [
        ".gitmodules",
        ".GITMODULES",
        "GITMOD~1",
        ".gitattributes",
        "GITATT~1",
        ".gitignore",
        "GITIGN~1",
        ".gitmodules.",
        ".git\u{206A}modules",
    ] {
        assert!(is_git_special_name(special), "{special:?} is special");
    }
    for not in [".git", "a.rs", ".github", "gitmodules", ".gitkeep"] {
        assert!(!is_git_special_name(not), "{not:?} is not special");
    }
}

/// ⛔2.1: pathspec magic is a literal name for the lexical check (Git receives `:(literal)`).
#[test]
fn pathspec_magic_is_a_literal_name_for_the_lexical_check() {
    for p in [
        ":(glob)**",
        ":!x",
        ":(top)a",
        ":(exclude)src",
        "src/:(glob)**",
        "*.rs",
        "a?b",
        "[a]b",
    ] {
        expect_unix_ok(p);
    }
}

#[cfg(not(windows))]
fn expect_unix_ok(p: &str) {
    expect(p, Ok(()));
}

// ':' is refused in any position on Windows (2.5); there the pathspec cases are not lexical.
#[cfg(windows)]
fn expect_unix_ok(p: &str) {
    if !p.contains(':') {
        expect(p, Ok(()));
    }
}

#[test]
fn too_many_paths_are_refused_before_looking_at_any() {
    assert_eq!(MAX_COMMIT_PATHS, 256);
    let ok: Vec<String> = (0..256).map(|i| format!("f/{i}.rs")).collect();
    assert_eq!(check_commit_paths(&ok), Ok(()), "256 paths pass");

    // 257 valid paths
    let mut many = ok.clone();
    many.push("f/256.rs".into());
    assert_eq!(check_commit_paths(&many), Err((0, PathRefusal::TooMany)));

    // 257 where the first is invalid: too-many still wins, nothing is looked at
    let mut bad_first: Vec<String> = vec!["/etc/passwd".into()];
    bad_first.extend((0..256).map(|i| format!("f/{i}.rs")));
    assert_eq!(
        check_commit_paths(&bad_first),
        Err((0, PathRefusal::TooMany)),
        "too-many must be decided before looking at any path"
    );
}

#[test]
fn the_first_refused_path_rejects_all_with_its_index() {
    let paths: Vec<String> = ["a.rs", "b/c.rs", "../x", "/abs"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(
        check_commit_paths(&paths),
        Err((2, PathRefusal::Outside)),
        "the first path that fails decides, with its index"
    );
    assert_eq!(
        check_commit_paths(&["ok".to_string(), "-f".to_string()]),
        Err((1, PathRefusal::Invalid))
    );
    assert_eq!(check_commit_paths(&["a".to_string()]), Ok(()));
}

#[test]
fn path_refusal_codes_are_kebab() {
    for (r, s) in [
        (PathRefusal::Invalid, "invalid"),
        (PathRefusal::Outside, "outside"),
        (PathRefusal::TooMany, "too-many"),
    ] {
        assert_eq!(serde_json::to_value(r).unwrap(), serde_json::json!(s));
    }
}

#[cfg(windows)]
#[test]
fn windows_forms_are_invalid() {
    for p in [
        "a:b",
        "a/b:stream",
        "dir./x",
        "dir /x",
        "file.",
        "file ",
        "CON",
        "con.txt",
        "src/NUL",
        "Aux.rs",
        "COM1",
        "lpt9.log",
        "PRN",
    ] {
        expect(p, Err(PathRefusal::Invalid));
    }
}
