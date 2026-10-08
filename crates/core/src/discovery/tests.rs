use std::fs;
use std::path::{Path, PathBuf};

use gitraptor_api::discovery::{BroadReason, RootRejection};

use super::*;

/// A fake repo: `.git` with `HEAD`, all the detection reads.
fn repo(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    dir.to_path_buf()
}

/// A linked worktree of `main` named `name`, at `dir`.
fn linked(main: &Path, name: &str, dir: &Path) {
    let gitdir = main.join(".git/worktrees").join(name);
    fs::create_dir_all(&gitdir).unwrap();
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
}

fn canonical(path: &Path) -> PathBuf {
    gitraptor_git::paths::canonicalize(path).unwrap()
}

fn names(listing: &Listing) -> Vec<&str> {
    listing.found.iter().map(|f| f.name.as_str()).collect()
}

fn reason(path: &Path, ctx: &RootContext) -> RootRejection {
    validate_root(path, ctx).unwrap_err().reason
}

#[test]
fn discovery_lists_the_repos_of_the_first_level_only() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    repo(&root.join("shop"));
    repo(&root.join("api"));
    // Second level: not looked at.
    repo(&root.join("clientes/acme"));
    // Not repos.
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::write(root.join("README"), "x").unwrap();
    // A clone in progress: `.git` without `HEAD`.
    fs::create_dir_all(root.join("cloning/.git")).unwrap();

    let listing = list_first_level(root, false).unwrap();
    assert_eq!(names(&listing), ["api", "shop"]);
    assert!(!listing.truncated);
    assert_eq!(
        listing.found[1].key_path,
        normalize_common_dir(&root.join("shop/.git"))
            .unwrap()
            .key_path
    );
}

#[test]
fn discovery_a_linked_worktree_shares_the_key_of_its_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let shop = repo(&root.join("shop"));
    linked(&shop, "feat", &root.join("shop-feat"));
    // A worktree of a repo outside the root is its repo's candidate.
    let outside = tempfile::tempdir().unwrap();
    let other = repo(&outside.path().join("other"));
    linked(&other, "w", &root.join("other-w"));

    let listing = list_first_level(root, false).unwrap();
    assert_eq!(names(&listing), ["other-w", "shop"]);
    assert_eq!(
        listing.found[0].key_path,
        normalize_common_dir(&other.join(".git")).unwrap().key_path
    );
}

#[test]
fn discovery_skips_hidden_entries_links_and_oversized_gitdir_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    repo(&root.join(".oh-my-zsh"));
    repo(&root.join("kept"));
    let outside = tempfile::tempdir().unwrap();
    let elsewhere = repo(&outside.path().join("elsewhere"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, root.join("linked")).unwrap();
    #[cfg(windows)]
    let _ = std::os::windows::fs::symlink_dir(&elsewhere, root.join("linked"));
    fs::create_dir_all(root.join("big")).unwrap();
    fs::write(
        root.join("big/.git"),
        format!(
            "gitdir: {}{}",
            elsewhere.join(".git").display(),
            " ".repeat(5000)
        ),
    )
    .unwrap();

    let listing = list_first_level(root, false).unwrap();
    assert_eq!(names(&listing), ["kept"]);
}

#[test]
fn discovery_the_home_root_skips_the_os_exclusions() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    repo(&home.join("dotlab"));
    for name in HOME_EXCLUDED {
        repo(&home.join(name));
    }
    assert_eq!(names(&list_first_level(home, true).unwrap()), ["dotlab"]);
    // The exclusions apply only to the home folder itself.
    assert_eq!(
        list_first_level(home, false).unwrap().found.len(),
        HOME_EXCLUDED.len() + 1
    );
}

#[test]
fn discovery_the_first_level_is_truncated_at_the_limit() {
    let tmp = tempfile::tempdir().unwrap();
    for n in 0..=MAX_ENTRIES {
        fs::write(tmp.path().join(format!("f{n}")), "").unwrap();
    }
    let listing = list_first_level(tmp.path(), false).unwrap();
    assert!(listing.truncated);
    assert!(listing.found.is_empty());
}

#[test]
fn discovery_a_normal_folder_is_a_root() {
    let tmp = tempfile::tempdir().unwrap();
    let code = tmp.path().join("code");
    repo(&code.join("shop"));
    let root = validate_root(&code, &RootContext::default()).unwrap();
    assert_eq!(root.path, canonical(&code));
    assert_eq!(root.broad, None);
    assert_eq!(root.entries, 1);
}

#[test]
fn discovery_invalid_roots_are_rejected_with_their_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    let home = base.join("users/rene");
    fs::create_dir_all(&home).unwrap();
    let profile = base.join("profile");
    fs::create_dir_all(profile.join("data")).unwrap();
    let ctx = RootContext {
        home: Some(home.clone()),
        profile: vec![profile.clone()],
    };
    let shop = repo(&base.join("shop"));
    fs::create_dir_all(shop.join("src")).unwrap();
    fs::write(base.join("file"), "x").unwrap();

    assert_eq!(
        reason(Path::new("relative"), &ctx),
        RootRejection::NotAbsolute
    );
    assert_eq!(reason(&base.join("missing"), &ctx), RootRejection::Missing);
    assert_eq!(
        reason(&base.join("file"), &ctx),
        RootRejection::NotADirectory
    );
    assert_eq!(
        reason(&base.join("users"), &ctx),
        RootRejection::HomeAncestor
    );
    assert_eq!(reason(&shop, &ctx), RootRejection::InsideRepo);
    assert_eq!(reason(&shop.join("src"), &ctx), RootRejection::InsideRepo);
    assert_eq!(reason(&profile.join("data"), &ctx), RootRejection::Profile);
    #[cfg(unix)]
    assert_eq!(reason(Path::new("/"), &ctx), RootRejection::FilesystemRoot);
}

#[cfg(unix)]
#[test]
fn discovery_a_symlinked_root_is_rejected_with_its_real_path() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    fs::create_dir_all(&real).unwrap();
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let err = validate_root(&link, &RootContext::default()).unwrap_err();
    assert_eq!(err.reason, RootRejection::Symlink);
    assert_eq!(err.real_path.as_deref(), canonical(&real).to_str());
}

#[test]
fn discovery_a_home_with_dotfiles_repo_is_inside_a_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let home = repo(&tmp.path().join("home"));
    let ctx = RootContext {
        home: Some(home.clone()),
        profile: Vec::new(),
    };
    assert_eq!(reason(&home, &ctx), RootRejection::InsideRepo);
}

#[test]
fn discovery_broad_roots_say_why() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let ctx = RootContext {
        home: Some(home.clone()),
        profile: Vec::new(),
    };
    let root = validate_root(&home, &ctx).unwrap();
    assert_eq!(root.broad, Some(BroadReason::Home));

    let wide = tmp.path().join("wide");
    fs::create_dir_all(&wide).unwrap();
    for n in 0..=BROAD_ENTRIES {
        fs::write(wide.join(format!("f{n}")), "").unwrap();
    }
    let root = validate_root(&wide, &ctx).unwrap();
    assert_eq!(root.broad, Some(BroadReason::Entries));
    assert_eq!(root.entries as usize, BROAD_ENTRIES + 1);
}

#[test]
fn discovery_reads_nothing_but_the_git_marker() {
    // A hostile repo: hooks and config that would leave a marker if anything
    // ran them. Detection never opens the repo.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let canary = repo(&root.join("canary"));
    let marker = tmp.path().join("marker");
    fs::write(
        canary.join(".git/config"),
        format!("[core]\n\tfsmonitor = touch {}\n", marker.display()),
    )
    .unwrap();
    assert_eq!(names(&list_first_level(root, false).unwrap()), ["canary"]);
    assert!(!marker.exists());
}
