//! DSK002-PIDPATH (TS-DSK-002 T003, B-1): the path of the executable of a pid, behind the safe
//! API of `gitraptor-macsys` (the FFI stays in a private `ffi_*` module). macOS only.
#![cfg(target_os = "macos")]

use gitraptor_macsys::process::executable_path;

#[test]
fn dsk002_pidpath_this_process_is_its_canonical_current_exe() {
    let expected = std::env::current_exe().unwrap().canonicalize().unwrap();
    let got = executable_path(std::process::id())
        .expect("DSK002-PIDPATH: executable_path gave no path for this process");
    assert_eq!(
        got.canonicalize().unwrap_or_else(|_| got.clone()),
        expected,
        "DSK002-PIDPATH: executable_path(own pid) is not the canonical current_exe"
    );
}

#[test]
fn dsk002_pidpath_a_pid_that_does_not_exist_has_no_path() {
    // Above the highest pid macOS hands out (99 998).
    let missing = 999_999;
    assert_eq!(
        executable_path(missing),
        None,
        "DSK002-PIDPATH: a pid that does not exist got a path"
    );
}
