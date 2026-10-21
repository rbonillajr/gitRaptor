// TS-CKP-001 T004: line ranges of the conflict blocks of a merged blob, in the coordinates of each
// side. Pure function over bytes: no repository is touched.

use gitraptor_git::dry_merge::{DryMergeLimits, LineRange, hunk_ranges};

fn r(start: u32, len: u32) -> LineRange {
    LineRange { start, len }
}

fn lines(prefix: &str, from: u32, to: u32) -> String {
    (from..=to).map(|i| format!("{prefix}{i}\n")).collect()
}

/// A conflict block with markers of `size` characters.
fn block(size: usize, ours: &str, theirs: &str) -> String {
    format!(
        "{} ours\n{ours}{}\n{theirs}{} theirs\n",
        "<".repeat(size),
        "=".repeat(size),
        ">".repeat(size)
    )
}

fn run(
    merged: &str,
    ours: &str,
    theirs: &str,
    limits: &DryMergeLimits,
) -> (Vec<LineRange>, Vec<LineRange>, bool) {
    hunk_ranges(
        merged.as_bytes(),
        ours.as_bytes(),
        theirs.as_bytes(),
        limits,
    )
}

mod marker {
    use super::*;

    /// Lines that look like markers but have another length, all identical on every side.
    fn mimic(size: usize) -> String {
        format!(
            "{} not a marker\nsome text\n{}\nmore text\n{} nor this\n",
            "<".repeat(size),
            "=".repeat(size),
            ">".repeat(size)
        )
    }

    #[test]
    fn a_line_that_mimics_a_short_marker_is_content() {
        let limits = DryMergeLimits::default();
        let text = format!("head\n{}tail\n", mimic(7));
        let (ours, theirs, truncated) = run(&text, &text, &text, &limits);
        assert!(ours.is_empty() && theirs.is_empty() && !truncated);
    }

    #[test]
    fn ckp001_marker_a_line_that_mimics_a_longer_marker_is_content() {
        let limits = DryMergeLimits::default();
        assert_eq!(limits.marker_size, 32);
        let text = format!("head\n{}tail\n", mimic(40));
        let (ours, theirs, truncated) = run(&text, &text, &text, &limits);
        assert!(ours.is_empty() && theirs.is_empty() && !truncated);
    }

    #[test]
    fn ckp001_marker_an_indented_marker_is_content() {
        let limits = DryMergeLimits::default();
        let text = format!(" {} x\nbody\n {}\n", "<".repeat(32), "=".repeat(32));
        let (ours, theirs, _) = run(&text, &text, &text, &limits);
        assert!(ours.is_empty() && theirs.is_empty());
    }

    #[test]
    fn ckp001_marker_mimic_lines_do_not_hide_the_real_block() {
        let limits = DryMergeLimits::default();
        let common = mimic(7);
        let ours = format!("{common}keep\nO\ntail\n");
        let theirs = format!("{common}keep\nT\ntail\n");
        let merged = format!("{common}keep\n{}tail\n", block(32, "O\n", "T\n"));
        let (o, t, truncated) = run(&merged, &ours, &theirs, &limits);
        assert_eq!((o, t, truncated), (vec![r(7, 1)], vec![r(7, 1)], false));
    }

    #[test]
    fn ckp001_marker_a_block_of_exactly_marker_size_is_found() {
        let limits = DryMergeLimits::default();
        let ours = "l1\nl2\nO\nl4\n";
        let theirs = "l1\nl2\nT\nl4\n";
        let merged = format!("l1\nl2\n{}l4\n", block(32, "O\n", "T\n"));
        let (o, t, _) = run(&merged, ours, theirs, &limits);
        assert_eq!((o, t), (vec![r(3, 1)], vec![r(3, 1)]));
    }
}

mod join {
    use super::*;

    /// Two blocks with `gap` unchanged lines between them (the same on both sides).
    fn two_blocks(gap: u32) -> (String, String, String) {
        let mid = lines("mid", 1, gap);
        let ours = format!("head\nO1\n{mid}O2\ntail\n");
        let theirs = format!("head\nT1\n{mid}T2\ntail\n");
        let merged = format!(
            "head\n{}{mid}{}tail\n",
            block(32, "O1\n", "T1\n"),
            block(32, "O2\n", "T2\n")
        );
        (merged, ours, theirs)
    }

    #[test]
    fn blocks_three_lines_apart_are_joined() {
        let limits = DryMergeLimits::default();
        assert_eq!(limits.hunk_join_gap, 3);
        let (merged, ours, theirs) = two_blocks(3);
        let (o, t, truncated) = run(&merged, &ours, &theirs, &limits);
        // Lines 2 to 6 on each side: block, three lines, block.
        assert_eq!((o, t, truncated), (vec![r(2, 5)], vec![r(2, 5)], false));
    }

    #[test]
    fn ckp001_join_blocks_four_lines_apart_stay_two() {
        let limits = DryMergeLimits::default();
        let (merged, ours, theirs) = two_blocks(4);
        let (o, t, _) = run(&merged, &ours, &theirs, &limits);
        assert_eq!(o, vec![r(2, 1), r(7, 1)]);
        assert_eq!(t, vec![r(2, 1), r(7, 1)]);
    }

    #[test]
    fn ckp001_join_blocks_adjacent_are_joined() {
        let limits = DryMergeLimits::default();
        let (merged, ours, theirs) = two_blocks(0);
        let (o, t, _) = run(&merged, &ours, &theirs, &limits);
        assert_eq!((o, t), (vec![r(2, 2)], vec![r(2, 2)]));
    }
}

mod sides {
    use super::*;

    #[test]
    fn ckp001_sides_ranges_use_the_coordinates_of_each_side() {
        let limits = DryMergeLimits::default();
        // Ours added two lines on top that merge cleanly: they shift ours, not theirs.
        let ours = "n1\nn2\nl1\nl2\nO\nl4\n";
        let theirs = "l1\nl2\nT\nl4\n";
        let merged = format!("n1\nn2\nl1\nl2\n{}l4\n", block(32, "O\n", "T\n"));
        let (o, t, truncated) = run(&merged, ours, theirs, &limits);
        assert_eq!(o, vec![r(5, 1)]);
        assert_eq!(t, vec![r(3, 1)]);
        assert!(!truncated);
    }

    #[test]
    fn ckp001_sides_each_side_has_its_own_length() {
        let limits = DryMergeLimits::default();
        let ours = "l1\nO1\nO2\nl3\n";
        let theirs = "l1\nT1\nT2\nT3\nl3\n";
        let merged = format!("l1\n{}l3\n", block(32, "O1\nO2\n", "T1\nT2\nT3\n"));
        let (o, t, _) = run(&merged, ours, theirs, &limits);
        assert_eq!((o, t), (vec![r(2, 2)], vec![r(2, 3)]));
    }

    #[test]
    fn ckp001_sides_resolved_parts_of_theirs_shift_ours() {
        let limits = DryMergeLimits::default();
        // Theirs added a line on top; ours did not.
        let ours = "l1\nl2\nO\nl4\n";
        let theirs = "n1\nl1\nl2\nT\nl4\n";
        let merged = format!("n1\nl1\nl2\n{}l4\n", block(32, "O\n", "T\n"));
        let (o, t, _) = run(&merged, ours, theirs, &limits);
        assert_eq!((o, t), (vec![r(3, 1)], vec![r(4, 1)]));
    }

    #[test]
    fn ckp001_sides_past_the_hunk_limit_it_cuts_and_marks_truncated() {
        let limits = DryMergeLimits {
            max_hunks_per_file: 2,
            ..DryMergeLimits::default()
        };
        let sep = lines("sep", 1, 6);
        let ours = format!("O1\n{sep}O2\n{sep}O3\n");
        let theirs = format!("T1\n{sep}T2\n{sep}T3\n");
        let merged = format!(
            "{}{sep}{}{sep}{}",
            block(32, "O1\n", "T1\n"),
            block(32, "O2\n", "T2\n"),
            block(32, "O3\n", "T3\n")
        );
        let (o, t, truncated) = run(&merged, &ours, &theirs, &limits);
        assert_eq!(o, vec![r(1, 1), r(8, 1)]);
        assert_eq!(t, vec![r(1, 1), r(8, 1)]);
        assert!(truncated);
    }

    #[test]
    fn ckp001_sides_at_the_hunk_limit_is_not_truncated() {
        let limits = DryMergeLimits {
            max_hunks_per_file: 2,
            ..DryMergeLimits::default()
        };
        let sep = lines("sep", 1, 6);
        let ours = format!("O1\n{sep}O2\n");
        let theirs = format!("T1\n{sep}T2\n");
        let merged = format!(
            "{}{sep}{}",
            block(32, "O1\n", "T1\n"),
            block(32, "O2\n", "T2\n")
        );
        let (o, _, truncated) = run(&merged, &ours, &theirs, &limits);
        assert_eq!(o.len(), 2);
        assert!(!truncated);
    }
}
