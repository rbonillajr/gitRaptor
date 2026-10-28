//! Criterion tests of T009 (CKPB-API-SHAPE, CKPB-API-FIT, CKPB-API-LIST): the wire shape of the
//! predictor's publication, how it fits the frame and the page, and the `prediction.list`
//! parameters. Written against the stubs of DS-TS-CKP-001 § Contrato de API.

use std::collections::HashSet;
use std::path::Path;

use serde_json::{Value, json};

use super::*;
use crate::framing::MAX_MESSAGE_BYTES;
use crate::messages::{BaseBranchView, BaseStatusView, RepoStateView, RepoView};

/// A worktree root made of `len` bytes that JSON escapes at six bytes each (control characters),
/// told apart by a numeric prefix.
fn heavy_root(n: usize, len: usize) -> Untrusted {
    let prefix = format!("{n:03}");
    Untrusted::new(format!("{prefix}{}", "\u{1}".repeat(len - prefix.len())))
}

fn side(root: &str) -> PairSideView {
    PairSideView::Worktree {
        root: Untrusted::new(root),
    }
}

fn summary(repo: &str, a: &str, b: PairSideView, state: PairStateView) -> PairSummary {
    let b_path = match &b {
        PairSideView::Worktree { root } => Some(root.raw().to_owned()),
        PairSideView::Base {} => None,
    };
    let id = pair_id(repo, Path::new(a), b_path.as_deref().map(Path::new));
    let (level, reason) = match state {
        PairStateView::Current | PairStateView::Recalculating => (Some(PairLevel::Overlap), None),
        PairStateView::Pending => (None, Some(PairReason::PowerSaving)),
        PairStateView::NotComputable => (None, Some(PairReason::LimitExceeded)),
        PairStateView::Calculating => (None, None),
    };
    PairSummary {
        pair_id: id,
        rev: 7,
        a: side(a),
        b,
        state,
        reason,
        level,
        computed_at_utc_ms: level.map(|_| 1_793_091_600_000),
        overlap_files: 1,
        overlap_partial: false,
        conflict_files: 0,
        conflict_truncated: false,
    }
}

fn repo_view(prediction: Option<PredictionSummary>) -> RepoView {
    RepoView {
        repo_id: "r1".into(),
        state: RepoStateView::Observed,
        path: Untrusted::new("/w/.git"),
        base: BaseBranchView {
            name: None,
            status: BaseStatusView::Invalid,
        },
        worktrees: Vec::new(),
        fetched_utc_ms: None,
        tier: None,
        checked_utc_ms: None,
        kept_temps: None,
        prediction,
    }
}

fn detail(n: usize, overlap: Vec<Untrusted>, conflict: Option<ConflictView>) -> PairDetail {
    PairDetail {
        summary: summary(
            "r1",
            &format!("/w/a{n:03}"),
            PairSideView::Base {},
            PairStateView::Current,
        ),
        overlap,
        conflict,
    }
}

fn plain(n: usize, size: usize) -> Untrusted {
    Untrusted::new(format!("{n:03}{}", "a".repeat(size - 3)))
}

fn page_bytes(page: &PredictionPage) -> usize {
    serde_json::to_vec(page).expect("serialize page").len()
}

mod shape {
    use super::*;

    #[test]
    fn level_absent_outside_current_and_recalculating() {
        let states = [
            (PairStateView::Calculating, false),
            (PairStateView::Current, true),
            (PairStateView::Recalculating, true),
            (PairStateView::Pending, false),
            (PairStateView::NotComputable, false),
        ];
        for (state, has_level) in states {
            let pair = summary("r1", "/w/a", PairSideView::Base {}, state);
            let value = serde_json::to_value(&pair).unwrap();
            assert_eq!(
                value.get("level").is_some(),
                has_level,
                "level must be present only in current and recalculating, state {state:?}: {value}"
            );
            let reason = value.get("reason").is_some();
            assert_eq!(
                reason,
                matches!(state, PairStateView::Pending | PairStateView::NotComputable),
                "reason must be present only in pending and not-computable, state {state:?}"
            );
        }
        // The pair id the summary carries is the one `pair_id` computes.
        let pair = summary(
            "r1",
            "/w/a",
            PairSideView::Base {},
            PairStateView::Calculating,
        );
        assert!(is_pair_id(&pair.pair_id), "pair_id must be a pair id");
    }

    #[test]
    fn wire_names_are_kebab_case() {
        let pairs = vec![
            summary(
                "r1",
                "/w/a",
                PairSideView::Base {},
                PairStateView::Calculating,
            ),
            summary("r1", "/w/a", PairSideView::Base {}, PairStateView::Current),
            summary(
                "r1",
                "/w/b",
                PairSideView::Base {},
                PairStateView::Recalculating,
            ),
            summary("r1", "/w/c", PairSideView::Base {}, PairStateView::Pending),
            summary(
                "r1",
                "/w/d",
                PairSideView::Base {},
                PairStateView::NotComputable,
            ),
        ];
        let published = fit_summary(pairs, MAX_MESSAGE_BYTES);
        let value = serde_json::to_value(&published).unwrap();
        assert_eq!(
            value["limits"],
            json!([
                "overlap-includes-uncommitted",
                "conflict-committed-only",
                "no-merge-drivers-or-attributes",
                "rename-limit",
                "insertions-may-merge",
                "joined-hunks",
                "pair-ignores-base-moves",
                "local-only"
            ]),
            "fit_summary must always publish the eight limits, kebab-case"
        );
        let states: Vec<&str> = value["pairs"]
            .as_array()
            .expect("pairs")
            .iter()
            .map(|p| p["state"].as_str().unwrap())
            .collect();
        assert_eq!(
            states,
            [
                "calculating",
                "current",
                "recalculating",
                "pending",
                "not-computable"
            ],
            "fit_summary must keep the pairs in order"
        );
        assert_eq!(value["pairs"][3]["reason"], "power-saving");
        assert_eq!(value["pairs"][4]["reason"], "limit-exceeded");
        assert_eq!(value["pairs"][1]["level"], "overlap");
        assert_eq!(value["pairs"][0]["b"], json!({ "base": {} }));
        assert_eq!(
            value["pairs"][0]["a"]["worktree"]["root"]["untrusted"],
            "/w/a"
        );

        // The rest of the names, one by one.
        let reasons = [
            (PairReason::BaseUnconfirmed, "base-unconfirmed"),
            (PairReason::OperationInProgress, "operation-in-progress"),
            (PairReason::PowerSaving, "power-saving"),
            (PairReason::Dormant, "dormant"),
            (PairReason::BaseMissing, "base-missing"),
            (PairReason::MissingObject, "missing-object"),
            (PairReason::Shallow, "shallow"),
            (PairReason::LimitExceeded, "limit-exceeded"),
            (PairReason::Error, "error"),
        ];
        for (reason, name) in reasons {
            assert_eq!(serde_json::to_value(reason).unwrap(), name);
        }
        let levels = [
            (PairLevel::Clean, "none"),
            (PairLevel::Overlap, "overlap"),
            (PairLevel::Conflict, "conflict"),
        ];
        for (level, name) in levels {
            assert_eq!(serde_json::to_value(level).unwrap(), name);
        }
        let bases = [
            (MergeBaseKind::Single, "single"),
            (MergeBaseKind::Virtual, "virtual"),
            (MergeBaseKind::Absent, "none"),
        ];
        for (kind, name) in bases {
            assert_eq!(serde_json::to_value(kind).unwrap(), name);
        }
        let kinds = [
            (ConflictKindView::Content, "content"),
            (ConflictKindView::AddAdd, "add-add"),
            (ConflictKindView::ModifyDelete, "modify-delete"),
            (ConflictKindView::Rename, "rename"),
            (ConflictKindView::DirectoryFile, "directory-file"),
            (ConflictKindView::Binary, "binary"),
            (ConflictKindView::Submodule, "submodule"),
        ];
        for (kind, name) in kinds {
            assert_eq!(serde_json::to_value(kind).unwrap(), name);
        }
        assert_eq!(LimitCode::ALL.len(), 8);
    }

    #[test]
    fn pair_id_is_deterministic_16_hex_and_distinct_per_pair() {
        let repo = "r1";
        let first = pair_id(repo, Path::new("/w/a"), Some(Path::new("/w/b")));
        let again = pair_id(repo, Path::new("/w/a"), Some(Path::new("/w/b")));
        assert_eq!(
            first, again,
            "pair_id must be deterministic, with no per-run salt"
        );
        assert_eq!(first.len(), PAIR_ID_LEN);
        assert!(
            first
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "pair_id must be lowercase hex: {first}"
        );
        assert!(is_pair_id(&first));

        // The 55 pairs of 10 worktrees: 10 against the base and 45 between worktrees.
        let roots: Vec<String> = (0..10).map(|i| format!("/w/wt{i}")).collect();
        let mut ids = HashSet::new();
        for (i, a) in roots.iter().enumerate() {
            ids.insert(pair_id(repo, Path::new(a), None));
            for b in &roots[i + 1..] {
                ids.insert(pair_id(repo, Path::new(a), Some(Path::new(b))));
            }
        }
        assert_eq!(ids.len(), 55, "every pair must have its own id");

        assert_ne!(
            pair_id("r1", Path::new("/w/a"), None),
            pair_id("r2", Path::new("/w/a"), None),
            "the repo id must take part in the hash"
        );
        assert_ne!(
            pair_id("r1", Path::new("/w/a"), None),
            pair_id("r1", Path::new("/w/a"), Some(Path::new("/w/b"))),
            "the base side must differ from a worktree side"
        );
    }

    #[test]
    fn capability_off_leaves_wire_unchanged() {
        let without = serde_json::to_value(repo_view(None)).unwrap();
        assert!(
            without.get("prediction").is_none(),
            "a RepoView without prediction must not carry the key: {without}"
        );
        let pair = summary("r1", "/w/a", PairSideView::Base {}, PairStateView::Current);
        let with =
            serde_json::to_value(repo_view(Some(fit_summary(vec![pair], MAX_MESSAGE_BYTES))))
                .unwrap();
        let Value::Object(mut with) = with else {
            panic!("a RepoView is an object")
        };
        assert!(
            with.remove("prediction").is_some(),
            "fit_summary must publish a prediction object"
        );
        assert_eq!(
            Value::Object(with),
            without,
            "with the capability off the snapshot must serialize as before"
        );
    }
}

mod fit {
    use super::*;

    #[test]
    fn snapshot_fits_one_mib_at_worst_case() {
        // 10 worktrees: 10 pairs against the base and 45 between worktrees, each root 4 KiB of
        // characters that JSON escapes at six bytes.
        let roots: Vec<Untrusted> = (0..10).map(|i| heavy_root(i, 4096)).collect();
        let mut pairs = Vec::new();
        for (i, a) in roots.iter().enumerate() {
            let mut base = summary("r1", a.raw(), PairSideView::Base {}, PairStateView::Current);
            base.a = PairSideView::Worktree { root: a.clone() };
            pairs.push(base);
            for b in &roots[i + 1..] {
                let mut p = summary(
                    "r1",
                    a.raw(),
                    PairSideView::Worktree { root: b.clone() },
                    PairStateView::Current,
                );
                p.a = PairSideView::Worktree { root: a.clone() };
                pairs.push(p);
            }
        }
        assert_eq!(pairs.len(), 55);
        let all_ids: Vec<String> = pairs.iter().map(|p| p.pair_id.clone()).collect();

        let overhead = serde_json::to_vec(&repo_view(None)).unwrap().len();
        let budget = MAX_MESSAGE_BYTES - overhead - 256;
        let published = fit_summary(pairs, budget);

        assert!(
            published.pairs_truncated,
            "55 worst-case pairs cannot fit one frame: pairs_truncated must be true"
        );
        assert!(
            !published.pairs.is_empty() && published.pairs.len() < 55,
            "fit_summary must keep some whole pairs and cut the rest, kept {}",
            published.pairs.len()
        );
        let kept: Vec<String> = published.pairs.iter().map(|p| p.pair_id.clone()).collect();
        assert_eq!(
            kept,
            all_ids[..kept.len()],
            "the pairs kept must be the first ones, in order"
        );
        assert_eq!(
            published.limits,
            LimitCode::ALL,
            "the eight limits always travel"
        );

        let framed = serde_json::to_vec(&repo_view(Some(published))).unwrap();
        assert!(
            framed.len() <= MAX_MESSAGE_BYTES,
            "the RepoView must fit MAX_MESSAGE_BYTES, it takes {} bytes",
            framed.len()
        );
    }

    #[test]
    fn snapshot_with_room_is_not_truncated() {
        let pairs: Vec<PairSummary> = (0..3)
            .map(|i| {
                summary(
                    "r1",
                    &format!("/w/a{i}"),
                    PairSideView::Base {},
                    PairStateView::Calculating,
                )
            })
            .collect();
        let published = fit_summary(pairs.clone(), MAX_MESSAGE_BYTES);
        assert!(!published.pairs_truncated, "everything fits: no truncation");
        assert_eq!(published.pairs, pairs);
        assert_eq!(published.limits, LimitCode::ALL);
    }

    #[test]
    fn list_page_caps_at_50_pairs_or_512_kib() {
        // By count: 120 small pairs make pages of 50, 50 and 20.
        let small: Vec<PairDetail> = (0..120)
            .map(|n| detail(n, vec![plain(n, 16)], None))
            .collect();
        let first = page(small.clone(), None);
        assert_eq!(
            first.pairs.len(),
            MAX_PAGE_PAIRS,
            "a page holds at most 50 pairs"
        );
        assert_eq!(first.limits, LimitCode::ALL);
        let cursor = first
            .next_cursor
            .clone()
            .expect("120 pairs leave more: next_cursor must be set");
        assert_eq!(first.pairs[0].summary.pair_id, small[0].summary.pair_id);

        let second = page(small.clone(), Some(&cursor));
        assert_eq!(second.pairs.len(), MAX_PAGE_PAIRS);
        assert_eq!(
            second.pairs[0].summary.pair_id, small[50].summary.pair_id,
            "the cursor must continue after the 50th pair"
        );
        let cursor = second.next_cursor.clone().expect("20 pairs remain");
        let third = page(small.clone(), Some(&cursor));
        assert_eq!(third.pairs.len(), 20);
        assert_eq!(third.next_cursor, None, "the last page has no next_cursor");

        // By bytes: pairs of ~100 KiB of overlap do not make a page of 50.
        let medium: Vec<PairDetail> = (0..20)
            .map(|n| detail(n, (0..200).map(|i| plain(i, 512)).collect(), None))
            .collect();
        let sized = page(medium, None);
        assert!(
            page_bytes(&sized) <= MAX_PAGE_BYTES,
            "a page must not pass 512 KiB, it takes {}",
            page_bytes(&sized)
        );
        assert!(
            !sized.pairs.is_empty() && sized.pairs.len() < 20,
            "20 pairs of ~100 KiB do not fit one page, took {}",
            sized.pairs.len()
        );
        assert!(
            sized.next_cursor.is_some(),
            "pairs remain: next_cursor must be set"
        );
    }

    #[test]
    fn a_pair_that_alone_overflows_the_page_is_cut_in_overlap_and_conflict_files() {
        let files: Vec<ConflictFileView> = (0..200)
            .map(|i| ConflictFileView {
                path: plain(i, 4096),
                kind: ConflictKindView::Content,
                ours: vec![LineRange { start: 1, len: 1 }],
                theirs: vec![LineRange { start: 1, len: 1 }],
                hunks_truncated: false,
                has_merge_rule: false,
            })
            .collect();
        let conflict = ConflictView {
            merge_base: MergeBaseKind::Single,
            files,
            truncated: false,
            rewrites_escalated: false,
        };
        let big = detail(
            0,
            (0..200).map(|i| plain(i, 4096)).collect(),
            Some(conflict),
        );
        let got = page(vec![big], None);
        assert_eq!(
            got.pairs.len(),
            1,
            "a pair that does not fit is cut, never dropped"
        );
        assert!(
            page_bytes(&got) <= MAX_PAGE_BYTES,
            "the page must not pass 512 KiB, it takes {}",
            page_bytes(&got)
        );
        let pair = &got.pairs[0];
        assert!(pair.overlap.len() < 200, "overlap must be cut");
        assert!(
            pair.summary.overlap_partial,
            "a cut overlap sets overlap_partial"
        );
        let conflict = pair.conflict.as_ref().expect("the conflict stays");
        assert!(conflict.files.len() < 200, "conflict.files must be cut");
        assert!(conflict.truncated, "cut conflict.files sets truncated");
    }
    fn heavy_pairs(n: usize) -> Vec<PairSummary> {
        (0..n)
            .map(|i| {
                let mut p = summary(
                    "r1",
                    &format!("/w/a{i}"),
                    PairSideView::Base {},
                    PairStateView::Current,
                );
                p.a = PairSideView::Worktree {
                    root: heavy_root(i, 4096),
                };
                p
            })
            .collect()
    }

    #[test]
    fn split_changed_cuts_an_oversized_event_into_batches_that_fit() {
        let pairs = heavy_pairs(55);
        let removed: Vec<String> = (0..3)
            .map(|i| pair_id("r1", Path::new(&format!("/w/gone{i}")), None))
            .collect();
        let changed = PredictionChanged {
            repo_id: "r1".into(),
            pairs: pairs.clone(),
            removed: removed.clone(),
        };
        assert!(
            serde_json::to_vec(&changed).unwrap().len() > MAX_MESSAGE_BYTES,
            "the fixture must not fit one frame"
        );
        let budget = MAX_MESSAGE_BYTES - 1024;
        let batches = split_changed(changed, budget);

        assert!(
            batches.len() > 1,
            "an oversized event must become several batches"
        );
        for (i, batch) in batches.iter().enumerate() {
            assert_eq!(batch.repo_id, "r1", "every batch carries the repo_id");
            let size = serde_json::to_vec(batch).unwrap().len();
            assert!(
                size <= budget,
                "batch {i} takes {size} bytes, over {budget}"
            );
            if i == 0 {
                assert_eq!(
                    batch.removed, removed,
                    "the removed ids go in the first batch"
                );
            } else {
                assert!(
                    batch.removed.is_empty(),
                    "only the first batch lists removed"
                );
            }
        }
        let joined: Vec<PairSummary> = batches.into_iter().flat_map(|b| b.pairs).collect();
        assert_eq!(joined, pairs, "the pairs keep their order and none is lost");
    }

    #[test]
    fn split_changed_leaves_an_event_that_fits_in_one_batch() {
        let changed = PredictionChanged {
            repo_id: "r1".into(),
            pairs: heavy_pairs(2),
            removed: Vec::new(),
        };
        let batches = split_changed(changed.clone(), MAX_MESSAGE_BYTES - 1024);
        assert_eq!(batches, vec![changed], "an event that fits is not split");
    }
}

mod list {
    use super::*;

    fn params(ids: Option<Vec<String>>, cursor: Option<&str>) -> ListPredictionParams {
        ListPredictionParams {
            repo_id: "r1".into(),
            worktree: None,
            pair_ids: ids,
            cursor: cursor.map(str::to_owned),
        }
    }

    fn valid_ids(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| pair_id("r1", Path::new(&format!("/w/a{i}")), None))
            .collect()
    }

    #[test]
    fn list_rejects_malformed_or_more_than_50_pair_ids() {
        assert_eq!(validate_list(&params(None, None)), Ok(()));
        assert_eq!(validate_list(&params(Some(valid_ids(50)), None)), Ok(()));
        assert_eq!(
            validate_list(&params(Some(valid_ids(51)), None)),
            Err(ListParamsError::TooManyPairIds),
            "more than 50 pair_ids must be refused"
        );
        for bad in [
            "",
            "xyz",
            "5a10e3c2f9b84d7",
            "5a10e3c2f9b84d77a",
            "5A10E3C2F9B84D77",
            "5a10e3c2f9b84d7g",
            "../../etc/passwd!",
        ] {
            let mut ids = valid_ids(2);
            ids.push(bad.to_owned());
            assert_eq!(
                validate_list(&params(Some(ids), None)),
                Err(ListParamsError::MalformedPairId),
                "the pair id {bad:?} is malformed and must be refused"
            );
            assert!(!is_pair_id(bad), "{bad:?} is not a pair id");
        }
    }

    #[test]
    fn list_rejects_a_foreign_cursor() {
        for bad in ["", "not-a-cursor", "0", "{\"a\":1}", "%%%%"] {
            assert_eq!(
                validate_list(&params(None, Some(bad))),
                Err(ListParamsError::MalformedCursor),
                "the cursor {bad:?} was not issued by page() and must be refused"
            );
        }
    }

    #[test]
    fn list_accepts_the_cursor_a_page_issued() {
        let details: Vec<PairDetail> = (0..60).map(|n| detail(n, Vec::new(), None)).collect();
        let first = page(details, None);
        let cursor = first.next_cursor.expect("60 pairs leave a second page");
        assert_eq!(validate_list(&params(None, Some(&cursor))), Ok(()));
    }
}
