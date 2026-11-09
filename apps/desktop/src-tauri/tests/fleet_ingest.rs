//! D4-INGEST (DS-US-DSK-004 T002): the worktree key and the `detached_at` rule of
//! `engine::ingest`. The key is FNV-1a 64 over the raw bytes, 16 lowercase hex digits; the known
//! vectors are the published ones of the algorithm.

#![cfg(unix)]

#[path = "support/fake_daemon.rs"]
mod fake_daemon;

use gitraptor_desktop::engine::ingest;

#[test]
fn key_is_fnv1a_64_in_16_lowercase_hex_of_the_raw_bytes() {
    for (raw, expected) in [
        ("", "cbf29ce484222325"),
        ("a", "af63dc4c8601ec8c"),
        ("foobar", "85944171f73967e8"),
    ] {
        assert_eq!(
            ingest::key(raw),
            expected,
            "D4-INGEST: key({raw:?}) must be the FNV-1a 64 vector"
        );
    }
    for raw in [
        "/w/tienda",
        "/w/wt/feat-pagos",
        "/w/wt/fe\u{202e}at",
        "/w/\u{1b}]0;x\u{7}",
    ] {
        let key = ingest::key(raw);
        assert_eq!(
            key.len(),
            16,
            "D4-INGEST: 16 digits for {raw:?}, got {key:?}"
        );
        assert!(
            key.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "D4-INGEST: lowercase hex for {raw:?}, got {key:?}"
        );
        assert_eq!(
            key,
            ingest::key(raw),
            "D4-INGEST: the same text always gives the same key"
        );
    }
    assert_ne!(
        ingest::key("/w/wt/fe\u{202e}at"),
        ingest::key("/w/wt/feat"),
        "D4-INGEST: the key is over the raw bytes, not over the sanitized text"
    );
    // Raw bytes, not chars: "é" is the two bytes c3 a9.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in [0xc3_u8, 0xa9] {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(
        ingest::key("é"),
        format!("{hash:016x}"),
        "D4-INGEST: the key hashes the UTF-8 bytes"
    );
}

#[test]
fn detached_at_passes_only_as_40_or_64_hex() {
    use gitraptor_api::messages::{HeadView, WorktreeStatus};

    let detached = |detached_at: Option<&str>| {
        let mut view = fake_daemon::worktree_view("/w/tienda", true, "main", 0, 0, 0);
        if let WorktreeStatus::Ready { head, .. } = &mut view.status {
            *head = HeadView::Detached;
        }
        view.detached_at = detached_at.map(str::to_owned);
        ingest::worktree(&view).detached_at
    };
    let sha1 = "0123456789abcdef0123456789abcdef01234567";
    let sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    assert_eq!(
        detached(Some(sha1)).as_deref(),
        Some(sha1),
        "D4-INGEST: 40 hex passes whole"
    );
    assert_eq!(
        detached(Some(sha256)).as_deref(),
        Some(sha256),
        "D4-INGEST: 64 hex passes whole"
    );
    assert_eq!(detached(None), None, "D4-INGEST: nothing stays nothing");
    for bad in [
        "",
        "abc1234",
        &sha1[..39],
        &format!("{sha1}0"),
        &sha256[..63],
        &format!("{sha256}0"),
        "g123456789abcdef0123456789abcdef01234567",
        "0123456789abcdef0123456789abcdef0123456\u{1b}",
        "0123456789abcdef0123456789abcdef0123456\u{202e}",
    ] {
        assert_eq!(
            detached(Some(bad)),
            None,
            "D4-INGEST: {bad:?} is not 40 or 64 hex and must not pass"
        );
    }
}
