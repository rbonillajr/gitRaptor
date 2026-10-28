//! Criterion tests of T006 protocol (CKPB-FRAME): the length-prefixed frames, the caps on what
//! the untrusted worker sends, and the numbers of § Valores numéricos.

use std::io::Cursor;
use std::time::Duration;

use gitraptor_git::dry_merge::{
    ConflictFile, ConflictKind, DryMergeLimits, DryMergeOutcome, LineRange, MergeBaseUsed,
};

use super::serve::{read_frame, validate_outcome, write_frame};
use super::{
    JOB_MEMORY_CAP, MAX_WORKER_FRAME, PAIR_DEADLINE, REWRITES_LIMIT_ESCALATED, WORKER_IDLE_EXIT,
    WORKER_RSS_POLL, WorkerLimits,
};

#[test]
fn frames_round_trip_in_order() {
    let frames: [&[u8]; 4] = [b"first", b"", b"{\"a\":1}", &[0, 255, 7]];
    let mut wire = Vec::new();
    for frame in frames {
        write_frame(&mut wire, frame, MAX_WORKER_FRAME).expect("a frame under the cap is written");
    }
    let mut input = Cursor::new(wire);
    for frame in frames {
        let got = read_frame(&mut input, MAX_WORKER_FRAME)
            .expect("a whole frame reads")
            .expect("a frame, not EOF");
        assert_eq!(got, frame);
    }
    assert_eq!(
        read_frame(&mut input, MAX_WORKER_FRAME).expect("EOF after a frame is clean"),
        None,
        "a clean EOF before a frame is None"
    );
}

#[test]
fn a_frame_has_a_four_byte_big_endian_length_prefix() {
    let mut wire = Vec::new();
    write_frame(&mut wire, b"hello", MAX_WORKER_FRAME).unwrap();
    assert_eq!(wire, [0, 0, 0, 5, b'h', b'e', b'l', b'l', b'o']);
}

#[test]
fn a_frame_of_exactly_the_cap_passes_and_one_more_byte_does_not() {
    let max = 64;
    let mut wire = Vec::new();
    write_frame(&mut wire, &vec![1; max], max).expect("a frame of the cap is written");
    let got = read_frame(&mut Cursor::new(wire), max).unwrap().unwrap();
    assert_eq!(got.len(), max);

    let mut wire = Vec::new();
    assert!(
        write_frame(&mut wire, &vec![1; max + 1], max).is_err(),
        "write_frame must refuse a frame over the cap"
    );
}

#[test]
fn a_frame_over_the_cap_is_an_error_on_read() {
    let max = 64;
    let mut wire = Vec::new();
    write_frame(&mut wire, &vec![1; max + 1], max + 1).unwrap();
    assert!(
        read_frame(&mut Cursor::new(wire), max).is_err(),
        "read_frame must refuse a frame whose length is over the cap"
    );

    // A header that claims 4 GiB must be refused, not allocated.
    let hostile = u32::MAX.to_be_bytes().to_vec();
    assert!(read_frame(&mut Cursor::new(hostile), MAX_WORKER_FRAME).is_err());
}

#[test]
fn a_truncated_frame_is_an_error_and_a_clean_eof_is_none() {
    let mut body_cut = Vec::new();
    write_frame(&mut body_cut, b"0123456789", MAX_WORKER_FRAME).unwrap();
    body_cut.truncate(body_cut.len() - 4);
    assert!(
        read_frame(&mut Cursor::new(body_cut), MAX_WORKER_FRAME).is_err(),
        "a body cut short is an error"
    );

    let header_cut = vec![0, 0];
    assert!(
        read_frame(&mut Cursor::new(header_cut), MAX_WORKER_FRAME).is_err(),
        "a header cut short is an error, not a clean EOF"
    );

    assert_eq!(
        read_frame(&mut Cursor::new(Vec::new()), MAX_WORKER_FRAME).unwrap(),
        None,
        "no bytes at all is a clean EOF"
    );
}

fn file(path: &str, ours: usize, theirs: usize) -> ConflictFile {
    ConflictFile {
        path: path.to_owned(),
        kind: ConflictKind::Content,
        ours: vec![LineRange { start: 1, len: 1 }; ours],
        theirs: vec![LineRange { start: 1, len: 1 }; theirs],
        hunks_truncated: false,
        has_merge_rule: false,
    }
}

fn outcome(files: Vec<ConflictFile>) -> DryMergeOutcome {
    DryMergeOutcome {
        merge_base: MergeBaseUsed::None,
        files,
        truncated: false,
        rewrites_escalated: false,
    }
}

#[test]
fn validate_outcome_rejects_more_files_than_the_limits_allow() {
    let limits = DryMergeLimits {
        max_files: 3,
        ..DryMergeLimits::default()
    };
    let files = |n: usize| (0..n).map(|i| file(&format!("f{i}.txt"), 1, 1)).collect();
    assert!(
        validate_outcome(&outcome(files(3)), &limits).is_ok(),
        "max_files files are within the limits"
    );
    assert!(
        validate_outcome(&outcome(files(4)), &limits).is_err(),
        "max_files + 1 files must be refused"
    );
    let default = DryMergeLimits::default();
    assert!(validate_outcome(&outcome(files(default.max_files + 1)), &default).is_err());
}

#[test]
fn validate_outcome_rejects_more_ranges_than_the_limits_allow() {
    let limits = DryMergeLimits {
        max_hunks_per_file: 2,
        ..DryMergeLimits::default()
    };
    assert!(validate_outcome(&outcome(vec![file("a", 2, 2)]), &limits).is_ok());
    assert!(
        validate_outcome(&outcome(vec![file("a", 3, 0)]), &limits).is_err(),
        "more ranges of ours than max_hunks_per_file must be refused"
    );
    assert!(
        validate_outcome(&outcome(vec![file("a", 0, 3)]), &limits).is_err(),
        "more ranges of theirs than max_hunks_per_file must be refused"
    );
}

#[test]
fn validate_outcome_accepts_a_clean_outcome() {
    assert!(validate_outcome(&outcome(Vec::new()), &DryMergeLimits::default()).is_ok());
}

#[test]
fn worker_limits_default_to_the_numbers_of_the_spec() {
    let d = WorkerLimits::default();
    assert_eq!(d.deadline, Duration::from_secs(2));
    assert_eq!(d.escalated_deadline, Duration::from_secs(2));
    assert_eq!(d.memory_cap, 512 * 1024 * 1024);
    assert_eq!(d.rss_poll, Duration::from_millis(20));
    assert_eq!(d.idle_exit, Duration::from_secs(60));
    assert_eq!(PAIR_DEADLINE, Duration::from_secs(2));
    assert_eq!(JOB_MEMORY_CAP, 512 * 1024 * 1024);
    assert_eq!(WORKER_RSS_POLL, Duration::from_millis(20));
    assert_eq!(WORKER_IDLE_EXIT, Duration::from_secs(60));
    assert_eq!(MAX_WORKER_FRAME, 4 * 1024 * 1024);
    assert_eq!(REWRITES_LIMIT_ESCALATED, 100_000);
}
