//! How a refused `snapshot` call because of the manual quota is told to the agent. Dedicated
//! file (wired from `messages.rs` by one line): the criteria's tests must not live in the
//! production file.
//!
//! The engine layer hands over `ToolRefusal { code, params }` where `params` carries what the
//! daemon said (`SnapshotQuotaData`: `window`, `retry_after_s`, `release_utc_ms`); the template
//! turns it into the tool's `params` (`release_utc` as "HH:MM UTC", computed without
//! dependencies from `release_utc_ms`).

use gitraptor_api::mcp_view::McpToolError;
use serde_json::json;

use super::{Lang, ToolRefusal, refusal};

/// The wait the write bucket answers (`rate-limited`): 60 s / 20 per minute.
const WRITE_BUCKET_WAIT_S: u64 = 3;

/// 2024-10-04 14:05:00 UTC.
const RELEASE_MS: i64 = 20_000 * 86_400_000 + (14 * 3_600 + 5 * 60) * 1_000;

#[test]
fn a_minute_quota_refusal_gives_the_real_wait() {
    let quota = ToolRefusal {
        code: McpToolError::QuotaExceeded,
        params: Some(json!({"window": "minute", "retry_after_s": 42})),
    };
    for lang in [Lang::En, Lang::Es] {
        let body = refusal(&quota, lang);
        assert_eq!(body["code"], "quota-exceeded", "{body}");
        assert_eq!(body["params"]["window"], "minute", "{body}");
        // The quota's own wait, not the 3 s of the write bucket.
        assert_eq!(body["params"]["retry_after_s"], 42, "{body}");
        assert_ne!(body["params"]["retry_after_s"], WRITE_BUCKET_WAIT_S);
        assert!(body["params"].get("release_utc").is_none(), "{body}");
        assert!(!body["message"].as_str().unwrap().is_empty(), "{body}");
        assert!(body["action"].as_str().unwrap().contains("42"), "{body}");
    }
}

#[test]
fn a_day_quota_refusal_gives_the_release_time() {
    let quota = ToolRefusal {
        code: McpToolError::QuotaExceeded,
        params: Some(json!({
            "window": "day",
            "retry_after_s": 3_600,
            "release_utc_ms": RELEASE_MS,
        })),
    };
    let en = refusal(&quota, Lang::En);
    assert_eq!(en["code"], "quota-exceeded", "{en}");
    assert_eq!(en["params"]["window"], "day", "{en}");
    assert_eq!(en["params"]["retry_after_s"], 3_600, "{en}");
    assert_eq!(en["params"]["release_utc"], "14:05 UTC", "{en}");
    // The raw instant is not part of the tool's answer.
    assert!(en["params"].get("release_utc_ms").is_none(), "{en}");

    let es = refusal(&quota, Lang::Es);
    assert_eq!(es["params"]["release_utc"], "14:05 UTC", "{es}");
    assert!(es["action"].as_str().unwrap().contains("14:05 UTC"), "{es}");
}
