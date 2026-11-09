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

mod prediction_ingest {
    use gitraptor_api::Untrusted;
    use gitraptor_api::display::{TEXT_MAX_CHARS, visible};
    use gitraptor_api::prediction::{
        ConflictFileView, ConflictKindView, ConflictView, LineRange, MergeBaseKind, PairDetail,
        PairLevel, PairSideView, PairStateView, PairSummary,
    };
    use gitraptor_desktop::engine::ingest;

    fn summary(computed: Option<i64>) -> PairSummary {
        PairSummary {
            pair_id: "0123456789abcdef".to_owned(),
            rev: 12,
            a: PairSideView::Worktree {
                root: Untrusted::new("/w/wt/feat-pagos"),
            },
            b: PairSideView::Base {},
            state: PairStateView::Current,
            reason: None,
            level: Some(PairLevel::Conflict),
            computed_at_utc_ms: computed,
            overlap_files: 1,
            overlap_partial: false,
            conflict_files: 1,
            conflict_truncated: false,
        }
    }

    fn detail(overlap: &[&str], files: &[&str]) -> PairDetail {
        PairDetail {
            summary: summary(Some(-5)),
            overlap: overlap.iter().map(|p| Untrusted::new(*p)).collect(),
            conflict: Some(ConflictView {
                merge_base: MergeBaseKind::Single,
                files: files
                    .iter()
                    .map(|p| ConflictFileView {
                        path: Untrusted::new(*p),
                        kind: ConflictKindView::Content,
                        ours: vec![LineRange { start: 40, len: 19 }],
                        theirs: vec![LineRange { start: 44, len: 18 }],
                        hunks_truncated: false,
                        has_merge_rule: true,
                    })
                    .collect(),
                truncated: true,
                rewrites_escalated: false,
            }),
        }
    }

    #[test]
    fn a_negative_computed_at_becomes_null() {
        assert_eq!(
            ingest::pair(&summary(Some(-1))).computed_at_utc_ms,
            None,
            "D8-WIRE: a negative computedAtUtcMs travels as null"
        );
        assert_eq!(
            ingest::pair(&summary(Some(1_700_000_300_000))).computed_at_utc_ms,
            Some(1_700_000_300_000),
            "D8-WIRE: a valid computedAtUtcMs is copied"
        );
        let dto = ingest::pair_detail(&detail(&["README.md"], &["pagos.rs"]));
        assert_eq!(
            dto.pair.computed_at_utc_ms, None,
            "D8-WIRE: the pair of a detail follows the same rule"
        );
        let conflict = dto
            .conflict
            .expect("D8-WIRE: pair_detail carries the conflict files");
        assert!(
            conflict.truncated,
            "D8-WIRE: the truncated flag travels as is"
        );
        let file = &conflict.files[0];
        assert_eq!(
            (
                file.ours[0].start,
                file.ours[0].len,
                file.theirs[0].start,
                file.theirs[0].len
            ),
            (40, 19, 44, 18),
            "D8-WIRE: the ranges travel as numbers"
        );
        assert!(file.has_merge_rule, "D8-WIRE: hasMergeRule travels as is");
    }

    #[test]
    fn pair_detail_paths_pass_through_display_visible() {
        let hostile = "src/\u{1b}]52;c;QUFB\u{7}pagos\u{202e}.rs";
        let long = format!("{}.rs", "d/".repeat(TEXT_MAX_CHARS));
        let dto = ingest::pair_detail(&detail(&[hostile, &long], &[hostile, &long]));
        let expected = [
            visible(hostile, TEXT_MAX_CHARS),
            visible(&long, TEXT_MAX_CHARS),
        ];
        assert_eq!(
            dto.overlap, expected,
            "D8-WIRE: overlap paths go through display::visible"
        );
        let conflict = dto
            .conflict
            .expect("D8-WIRE: pair_detail carries the conflict files");
        let paths: Vec<String> = conflict.files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(
            paths, expected,
            "D8-WIRE: conflict file paths go through display::visible"
        );
        for path in &paths {
            assert!(
                !path.contains('\u{1b}') && !path.contains('\u{202e}') && !path.contains('\u{7}'),
                "D8-WIRE: no raw escape or bidi control may reach the webview: {path:?}"
            );
        }
    }
}
