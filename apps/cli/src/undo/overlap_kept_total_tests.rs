//! The kept paths' count and "+K more" come from the real total the daemon sent, not from the
//! cut list. Child module of `undo::overlap`.

use super::*;
use crate::i18n::Tr;
use gitraptor_api::Untrusted;
use gitraptor_api::timemachine::KeptPath;

const EN: Tr = Tr { spanish: false };
const ES: Tr = Tr { spanish: true };

fn kept(n: usize) -> Vec<KeptPath> {
    (0..n)
        .map(|i| KeptPath {
            worktree: Untrusted::new("main"),
            path: Untrusted::new(format!("src/k{i:03}.rs")),
        })
        .collect()
}

#[test]
fn a_cut_list_counts_and_says_more_from_the_real_total() {
    for tr in [EN, ES] {
        let lines = kept_lines_of(tr, &kept(200), 537);
        let all = lines.join("\n");
        assert!(lines[0].contains("537"), "the real number: {all}");
        assert_eq!(
            lines.iter().filter(|l| l.contains("src/k")).count(),
            SHOWN,
            "{all}"
        );
        assert!(all.contains("+517"), "537 - 20 shown: {all}");
    }
}

#[test]
fn a_total_below_the_list_never_undercounts() {
    let lines = kept_lines_of(EN, &kept(25), 0);
    let all = lines.join("\n");
    assert!(lines[0].contains("25"), "{all}");
    assert!(all.contains("+5"), "{all}");
}
