//! The progress of a long `safe_commit`: a notice only when the request brought a
//! `progressToken`, with the seconds elapsed and no text.

use std::time::Duration;

use rmcp::model::{NumberOrString, ProgressToken};

use super::progress_notice;

#[test]
fn progress_is_sent_only_with_a_progress_token() {
    assert!(
        progress_notice(None, Duration::from_secs(5)).is_none(),
        "without a token nothing is sent"
    );
    let token = ProgressToken(NumberOrString::Number(7));
    let notice = progress_notice(Some(&token), Duration::from_millis(10_400))
        .expect("a token gets its notice");
    let wire = serde_json::to_value(&notice).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"progressToken": 7, "progress": 10.0}),
        "the seconds elapsed, without total or message: {wire}"
    );
}
