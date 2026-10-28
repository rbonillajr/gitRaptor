//! T002 (DS-TS-GRP-005): the work class of a thread follows what it declares, read from the OS
//! on macOS. Each test runs its checks on a thread of its own, so the harness threads keep
//! their class.

use gitraptor_core::work_class::{self, Pool, PoolClass, WorkClass};

#[cfg(target_os = "macos")]
#[test]
fn a_utility_thread_reads_utility_from_the_os() {
    let h = work_class::spawn(
        Pool::Maintenance,
        WorkClass::Utility,
        "t-wc-utility",
        work_class::current,
    )
    .unwrap();
    assert_eq!(
        h.join().unwrap(),
        Some(WorkClass::Utility),
        "spawn must put the thread in the class it declares"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_scoped_class_restores_the_previous_one() {
    work_class::spawn_unpooled(WorkClass::Default, "t-wc-scoped", || {
        work_class::set_current(WorkClass::Default);
        {
            let g = work_class::scoped(WorkClass::Utility);
            assert!(
                g.is_active(),
                "lowering from Default must make an active guard"
            );
            assert_eq!(
                work_class::current(),
                Some(WorkClass::Utility),
                "inside the scope the thread runs in Utility"
            );
        }
        assert_eq!(
            work_class::current(),
            Some(WorkClass::Default),
            "dropping the guard must restore the previous class"
        );
    })
    .unwrap()
    .join()
    .unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn scoped_never_raises() {
    work_class::spawn_unpooled(WorkClass::Background, "t-wc-noraise", || {
        work_class::set_current(WorkClass::Background);
        let g = work_class::scoped(WorkClass::Default);
        assert!(!g.is_active(), "a higher class must give an empty guard");
        assert_eq!(
            work_class::current(),
            Some(WorkClass::Background),
            "scoped must never raise the thread"
        );
        drop(g);
        assert_eq!(work_class::current(), Some(WorkClass::Background));
    })
    .unwrap()
    .join()
    .unwrap();
}

#[cfg(not(target_os = "macos"))]
#[test]
fn an_unsupported_platform_leaves_the_thread_untouched() {
    assert_eq!(work_class::support(), work_class::Support::Unavailable);
    assert_eq!(work_class::current(), None);
    assert_eq!(
        work_class::set_current(WorkClass::Utility),
        work_class::Outcome::Unavailable
    );
    assert!(!work_class::scoped(WorkClass::Utility).is_active());
}

#[cfg(target_os = "macos")]
#[test]
fn a_finished_pool_leaves_the_registry() {
    use std::sync::mpsc;
    // Two pools no other test of this binary uses: Capture (ends) and Predictor (panics).
    let (up_tx, up_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let ends = work_class::spawn(Pool::Capture, WorkClass::Utility, "t-wc-ends", move || {
        up_tx.send(()).unwrap();
        go_rx.recv().unwrap();
    })
    .unwrap();
    up_rx.recv().unwrap();
    assert!(
        work_class::pools().contains(&PoolClass {
            pool: Pool::Capture,
            class: WorkClass::Utility
        }),
        "a live pool thread must be listed"
    );
    go_tx.send(()).unwrap();
    ends.join().unwrap();
    assert!(
        !work_class::pools().iter().any(|p| p.pool == Pool::Capture),
        "a pool thread that ended must leave the registry"
    );

    let (up_tx, up_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let panics = work_class::spawn(
        Pool::Predictor,
        WorkClass::Utility,
        "t-wc-panics",
        move || {
            up_tx.send(()).unwrap();
            go_rx.recv().unwrap();
            panic!("a pool thread that panics (expected by the test)");
        },
    )
    .unwrap();
    up_rx.recv().unwrap();
    assert!(
        work_class::pools()
            .iter()
            .any(|p| p.pool == Pool::Predictor)
    );
    go_tx.send(()).unwrap();
    assert!(panics.join().is_err());
    assert!(
        !work_class::pools()
            .iter()
            .any(|p| p.pool == Pool::Predictor),
        "a pool thread that panicked must leave the registry"
    );
}
