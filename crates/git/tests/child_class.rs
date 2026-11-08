//! TS-GRP-005 T011 (GIT1, D9): the class of the `git` children of the invocation layer. A fake
//! `git` (a script in a temporary folder, like `cli_invoke.rs`) writes its own nice, which the
//! layer's `spawn_and_wait` launches through `resolve::resolve` (`git --version`). The call is
//! made from a fresh thread whose QoS the test sets; nothing touches the real repo (NFR-01).
//! Needs `gitraptor-macsys` among the dependencies of `gitraptor-git` on macOS (T011).
#![cfg(target_os = "macos")]

mod common;

use std::path::Path;

use common::{Fixture, script};
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_macsys::qos::{self, QosClass};

/// The nice the fake `git` reads from the OS for its own process, launched by the layer from a
/// thread of `class`.
fn child_nice_from(class: QosClass) -> i32 {
    let f = Fixture::new();
    let bin = f.root().join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    let out = f.root().join("nice-out");
    let git = bin.join("git");
    script(
        &git,
        &format!(
            "ps -o nice= -p $$ > '{o}'\necho 'git version 2.40.0'",
            o = out.display()
        ),
    );
    let invoker = f.invoker();
    let config = ResolveConfig {
        configured_path: Some(git.clone()),
        path_env: None,
        known_locations: vec![],
        shim_paths: vec![],
        toolchain_gits: vec![],
    };
    std::thread::scope(|s| {
        s.spawn(|| {
            qos::set_current(class).expect("the thread class must be set");
            let Resolution::Found { .. } = resolve::resolve(&config, &invoker) else {
                panic!("fake git not selected");
            };
        })
        .join()
        .unwrap();
    });
    read_nice(&out)
}

fn read_nice(out: &Path) -> i32 {
    std::fs::read_to_string(out)
        .expect("the fake git must have run and written its nice")
        .trim()
        .parse()
        .expect("nice is a number")
}

/// The nice this test process hands to its children.
fn inherited_nice() -> i32 {
    let out = std::process::Command::new("/bin/ps")
        .args(["-o", "nice=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[test]
fn a_git_child_of_a_background_thread_runs_at_nice_10() {
    assert_eq!(
        child_nice_from(QosClass::Background),
        10,
        "a git launched from a Background thread must run at nice 10 (lower_child in spawn_and_wait)"
    );
}

#[test]
fn a_git_child_of_a_utility_thread_runs_at_nice_5() {
    assert_eq!(
        child_nice_from(QosClass::Utility),
        5,
        "a git launched from a Utility thread must run at nice 5 (lower_child in spawn_and_wait)"
    );
}

#[test]
fn a_default_thread_installs_no_pre_exec() {
    assert_eq!(
        child_nice_from(QosClass::Default),
        inherited_nice(),
        "a git launched from a Default thread keeps the inherited priority"
    );
}
