//! T001 (DS-TS-GRP-005): QoS and I/O policy of the current thread, and of a child, read back
//! from the OS.
#![cfg(target_os = "macos")]

use std::process::Command;

use gitraptor_macsys::qos::{self, QosClass};

#[test]
fn set_current_utility_reads_back_utility_and_iopol_utility() {
    // A fresh thread: the test thread of the harness is not shared with other tests.
    std::thread::spawn(|| {
        qos::set_current(QosClass::Utility).expect("set_current(Utility) must succeed");
        assert_eq!(
            qos::current(),
            Some(QosClass::Utility),
            "the thread QoS must read back as Utility"
        );
        assert_eq!(
            qos::current_io(),
            Some(QosClass::Utility),
            "the thread I/O policy must read back as IOPOL_UTILITY"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn lower_child_background_runs_the_child_at_nice_10() {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "ps -o nice= -p $$"]);
    qos::lower_child(&mut cmd, QosClass::Background);
    let out = cmd.output().expect("the lowered child must start");
    assert!(out.status.success(), "the child must exit cleanly");
    let nice: i32 = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("ps prints the nice of the child");
    assert_eq!(nice, 10, "a Background child must run at nice 10");
}
