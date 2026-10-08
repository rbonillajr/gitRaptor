//! A label character that `check_snapshot_label` admits but the MCP escape replaces comes out
//! escaped: the check is not the only defence of what an agent reads.

use gitraptor_api::catalog::check_snapshot_label;
use gitraptor_api::mcp_view::for_mcp;
use serde_json::json;

#[test]
fn a_hidden_character_in_a_label_comes_out_escaped() {
    // U+0600 (ARABIC NUMBER SIGN) is a format character that renders as nothing in a line of
    // text: not a control, not in the label's hidden set, but in the escape's.
    let label = "before\u{600}after";
    check_snapshot_label(label).expect("the label check admits it");

    let mut value = json!({ "label": { "untrusted": label } });
    for_mcp(&mut value);

    assert_eq!(value["label"]["untrusted"], "before\u{FFFD}after");
}
