//! The FSEvents stream of `gitraptor-macsys` over temporary folders (ADR-GRP-010, Enmienda
//! 2026-10-08): exclusions, resumption, and the end of the callbacks.
#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_macsys::fsevents::{Event, MAX_EXCLUSIONS, SINCE_NOW, Stream, StreamError, flag};

const WAIT: Duration = Duration::from_secs(10);

type Seen = (PathBuf, u32);

/// A stream whose events (path, flags) arrive on a channel.
fn start(root: &Path, excl: &[PathBuf], since: u64) -> (Stream, Receiver<Seen>) {
    let (tx, rx) = channel();
    let tx: Mutex<Sender<Seen>> = Mutex::new(tx);
    let stream = Stream::start(
        root,
        excl,
        since,
        Box::new(move |events: &[Event<'_>]| {
            let tx = tx.lock().unwrap();
            for e in events {
                let _ = tx.send((e.path.to_path_buf(), e.flags));
            }
        }),
    )
    .expect("stream");
    (stream, rx)
}

/// Waits until `pred` holds for an event, or `WAIT` passes.
fn wait_for(rx: &Receiver<Seen>, mut pred: impl FnMut(&Path, u32) -> bool) -> bool {
    let end = Instant::now() + WAIT;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok((p, f)) if pred(&p, f) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn a_write_outside_the_excluded_folder_is_seen_and_one_inside_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    // `/var/folders/...` is a symlink to `/private/var/...`: the root is given as is.
    let root = tmp.path().join("répo-ñ");
    fs::create_dir_all(root.join("target")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let (stream, rx) = start(&root, &[root.join("target")], SINCE_NOW);
    assert_eq!(stream.exclusions().len(), 1);
    fs::write(root.join("target/a.o"), b"x").unwrap();
    fs::write(root.join("src/lib.rs"), b"x").unwrap();
    let mut all = Vec::new();
    assert!(wait_for(&rx, |p, f| {
        all.push((p.to_path_buf(), f));
        p.ends_with("src/lib.rs")
    }));
    stream.flush_sync();
    all.extend(rx.try_iter());
    assert!(
        all.iter().all(|(p, _)| !p.ends_with("target/a.o")),
        "an excluded folder reached the callback: {all:?}"
    );
}

#[test]
fn the_excluded_folder_is_seen_without_the_exclusion() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(root.join("target")).unwrap();
    let (_stream, rx) = start(&root, &[], SINCE_NOW);
    fs::write(root.join("target/a.o"), b"x").unwrap();
    assert!(wait_for(&rx, |p, _| p.ends_with("target/a.o")));
}

#[test]
fn more_than_eight_exclusions_are_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs: Vec<PathBuf> = (0..=MAX_EXCLUSIONS)
        .map(|i| {
            let d = tmp.path().join(format!("d{i}"));
            fs::create_dir_all(&d).unwrap();
            d
        })
        .collect();
    let r = Stream::start(tmp.path(), &dirs, SINCE_NOW, Box::new(|_| {}));
    assert_eq!(r.err(), Some(StreamError::TooManyExclusions));
}

#[test]
fn a_missing_root_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let r = Stream::start(&tmp.path().join("none"), &[], SINCE_NOW, Box::new(|_| {}));
    assert_eq!(r.err(), Some(StreamError::InvalidPath));
}

#[test]
fn a_stream_started_from_the_last_id_replays_what_the_old_one_missed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(&root).unwrap();
    let (old, rx) = start(&root, &[], SINCE_NOW);
    fs::write(root.join("before"), b"x").unwrap();
    assert!(wait_for(&rx, |p, _| p.ends_with("before")));
    old.flush_sync();
    let from = old.last_event_id();
    // The gap: written while no stream of ours is running.
    drop(old);
    fs::write(root.join("during-the-gap"), b"x").unwrap();
    let (new, rx) = start(&root, &[], from);
    assert!(wait_for(&rx, |p, _| p.ends_with("during-the-gap")));
    assert!(wait_for(&rx, |_, f| f & flag::HISTORY_DONE != 0));
    drop(new);
}

#[test]
fn no_callback_runs_after_the_stream_is_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(&root).unwrap();
    let count = Arc::new(Mutex::new(0usize));
    let seen = Arc::clone(&count);
    let stream = Stream::start(
        &root,
        &[],
        SINCE_NOW,
        Box::new(move |events: &[Event<'_>]| *seen.lock().unwrap() += events.len()),
    )
    .unwrap();
    fs::write(root.join("a"), b"x").unwrap();
    stream.flush_sync();
    drop(stream);
    let after_drop = *count.lock().unwrap();
    for i in 0..50 {
        fs::write(root.join(format!("late{i}")), b"x").unwrap();
    }
    // A negative: nothing arrives to wait for, so a short pause gives a late callback its time.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(*count.lock().unwrap(), after_drop);
}

/// E4 of the ADR: the exclusions also filter the history `since_when` replays (measured on
/// macOS 26), so a replacement stream does not pay for the churn of an excluded folder.
#[test]
fn exclusions_also_filter_the_replayed_history() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(root.join("target")).unwrap();
    let (old, rx) = start(&root, &[], SINCE_NOW);
    fs::write(root.join("before"), b"x").unwrap();
    assert!(wait_for(&rx, |p, _| p.ends_with("before")));
    old.flush_sync();
    let from = old.last_event_id();
    drop(old);
    fs::write(root.join("target/in-excluded"), b"x").unwrap();
    fs::write(root.join("outside"), b"x").unwrap();
    let (_new, rx) = start(&root, &[root.join("target")], from);
    let (mut outside, mut excluded) = (false, false);
    assert!(wait_for(&rx, |p, f| {
        outside |= p.ends_with("outside");
        excluded |= p.ends_with("target/in-excluded");
        f & flag::HISTORY_DONE != 0
    }));
    assert!(outside, "the history was not replayed");
    assert!(!excluded, "the excluded folder was replayed");
}

/// A symlink is never an exclusion: repointed, it would leave out another folder.
#[test]
fn a_symlink_is_not_taken_as_an_exclusion() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(root.join("real")).unwrap();
    std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
    let (stream, _rx) = start(&root, &[root.join("link"), root.join("real")], SINCE_NOW);
    assert_eq!(stream.exclusions().len(), 1);
    assert!(stream.exclusions()[0].ends_with("real"));
}
