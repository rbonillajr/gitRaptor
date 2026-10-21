//! Line ranges of the conflict blocks of a merged blob, in the coordinates of each side.
//!
//! The merged blob does not give them directly: the lines around a block may come from either
//! side. So the "ours view" (every block replaced by its ours lines) is mapped to the ours blob
//! with a line diff; the ours lines of a block are verbatim from ours, so the diff keeps them
//! as unchanged lines. Same for theirs.

use gix::diff::blob::{Algorithm, Diff, InternedInput};

use super::{DryMergeLimits, LineRange};

/// Ranges of the conflict blocks of `merged` on ours and on theirs, and whether they were cut.
///
/// Only a marker of exactly [`DryMergeLimits::marker_size`] at the start of a line counts:
/// longer markers (a virtual base's) and shorter ones are content. Blocks at most
/// [`DryMergeLimits::hunk_join_gap`] lines apart on both sides are joined into one, like Git
/// does. Past [`DryMergeLimits::max_hunks_per_file`] the list is cut and the flag is `true`.
///
/// Unbalanced markers, or a block line the diff cannot place in its side, give no ranges and
/// the flag `true`: a range is never guessed.
pub fn hunk_ranges(
    merged: &[u8],
    ours: &[u8],
    theirs: &[u8],
    limits: &DryMergeLimits,
) -> (Vec<LineRange>, Vec<LineRange>, bool) {
    let Some(blocks) = side_ranges(merged, ours, theirs, usize::from(limits.marker_len().get()))
    else {
        return (Vec::new(), Vec::new(), true);
    };
    let joined = join(blocks, limits.hunk_join_gap);
    let truncated = joined.len() > limits.max_hunks_per_file;
    let (o, t) = joined.into_iter().take(limits.max_hunks_per_file).unzip();
    (o, t, truncated)
}

fn lines(data: &[u8]) -> Vec<&[u8]> {
    data.split_inclusive(|b| *b == b'\n').collect()
}

/// A marker line: exactly `size` times `ch`, then a space, a line end or the end of the blob.
fn is_marker(line: &[u8], ch: u8, size: usize) -> bool {
    line.len() >= size
        && line[..size].iter().all(|b| *b == ch)
        && matches!(line.get(size), None | Some(b' ' | b'\n' | b'\r'))
}

#[derive(Default)]
struct Block {
    /// Indices of the block's lines in each view.
    ours: Vec<usize>,
    theirs: Vec<usize>,
    /// Position of the block in each view, for a side with no lines.
    ours_at: usize,
    theirs_at: usize,
}

enum Part {
    Outside,
    Ours,
    Base,
    Theirs,
}

fn side_ranges(
    merged: &[u8],
    ours: &[u8],
    theirs: &[u8],
    size: usize,
) -> Option<Vec<(LineRange, LineRange)>> {
    let mut ours_view: Vec<&[u8]> = Vec::new();
    let mut theirs_view: Vec<&[u8]> = Vec::new();
    let mut blocks = Vec::new();
    let mut part = Part::Outside;
    let mut cur = Block::default();
    for line in lines(merged) {
        match part {
            Part::Outside if is_marker(line, b'<', size) => {
                part = Part::Ours;
                cur = Block {
                    ours_at: ours_view.len(),
                    theirs_at: theirs_view.len(),
                    ..Block::default()
                };
            }
            Part::Outside => {
                ours_view.push(line);
                theirs_view.push(line);
            }
            Part::Ours if is_marker(line, b'|', size) => part = Part::Base,
            Part::Ours | Part::Base if is_marker(line, b'=', size) => part = Part::Theirs,
            Part::Ours => {
                cur.ours.push(ours_view.len());
                ours_view.push(line);
            }
            Part::Base => {}
            Part::Theirs if is_marker(line, b'>', size) => {
                part = Part::Outside;
                blocks.push(std::mem::take(&mut cur));
            }
            Part::Theirs => {
                cur.theirs.push(theirs_view.len());
                theirs_view.push(line);
            }
        }
    }
    if !matches!(part, Part::Outside) {
        return None;
    }
    let om = view_to_side(&ours_view.concat(), ours);
    let tm = view_to_side(&theirs_view.concat(), theirs);
    blocks
        .iter()
        .map(|b| {
            Some((
                range(&b.ours, b.ours_at, &om)?,
                range(&b.theirs, b.theirs_at, &tm)?,
            ))
        })
        .collect()
}

/// The range of a block's lines in its side; a side with no lines gives an empty range just
/// after the side's line before the block.
fn range(idx: &[usize], at: usize, map: &[Option<u32>]) -> Option<LineRange> {
    let (Some(first), Some(last)) = (idx.first(), idx.last()) else {
        let start = match at.checked_sub(1) {
            None => 1,
            Some(prev) => map.get(prev).copied().flatten()? + 2,
        };
        return Some(LineRange { start, len: 0 });
    };
    let first = map.get(*first).copied().flatten()?;
    let last = map.get(*last).copied().flatten()?;
    Some(LineRange {
        start: first + 1,
        len: last.checked_sub(first)? + 1,
    })
}

/// For every line of `view`, its 0-based line in `side` when the diff keeps it unchanged.
fn view_to_side(view: &[u8], side: &[u8]) -> Vec<Option<u32>> {
    let n = lines(view).len();
    let input = InternedInput::new(side, view);
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let mut map = vec![None; n];
    let (mut s, mut v) = (0u32, 0u32);
    let keep = |map: &mut Vec<Option<u32>>, s: &mut u32, v: &mut u32, until: u32| {
        while *v < until {
            if let Some(slot) = map.get_mut(*v as usize) {
                *slot = Some(*s);
            }
            *s += 1;
            *v += 1;
        }
    };
    for h in diff.hunks() {
        keep(&mut map, &mut s, &mut v, h.after.start);
        s = h.before.end;
        v = h.after.end;
    }
    keep(
        &mut map,
        &mut s,
        &mut v,
        u32::try_from(n).unwrap_or(u32::MAX),
    );
    map
}

/// Joins consecutive blocks whose gap is at most `gap` lines on both sides.
fn join(blocks: Vec<(LineRange, LineRange)>, gap: u32) -> Vec<(LineRange, LineRange)> {
    let mut out: Vec<(LineRange, LineRange)> = Vec::with_capacity(blocks.len());
    for (o, t) in blocks {
        if let Some((po, pt)) = out.last_mut()
            && distance(po, &o) <= u64::from(gap)
            && distance(pt, &t) <= u64::from(gap)
        {
            po.len = o.start.saturating_add(o.len).saturating_sub(po.start);
            pt.len = t.start.saturating_add(t.len).saturating_sub(pt.start);
            continue;
        }
        out.push((o, t));
    }
    out
}

/// Lines between the end of `prev` and the start of `next`.
fn distance(prev: &LineRange, next: &LineRange) -> u64 {
    u64::from(next.start).saturating_sub(u64::from(prev.start) + u64::from(prev.len))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(start: u32, len: u32) -> LineRange {
        LineRange { start, len }
    }

    #[test]
    fn a_marker_must_have_the_exact_size() {
        assert!(is_marker(b"<<<<<<< ours\n", b'<', 7));
        assert!(!is_marker(b"<<<<<<<< ours\n", b'<', 7));
        assert!(!is_marker(b"<<<<<<< not a marker\n", b'<', 32));
    }

    #[test]
    fn a_side_without_lines_gets_an_empty_range_after_its_previous_line() {
        let limits = DryMergeLimits {
            marker_size: 7,
            ..DryMergeLimits::default()
        };
        let merged = b"a\nb\n<<<<<<< o\n=======\nT\n>>>>>>> t\nc\n";
        let (o, t, cut) = hunk_ranges(merged, b"a\nb\nc\n", b"a\nb\nT\nc\n", &limits);
        assert_eq!((o, t, cut), (vec![r(3, 0)], vec![r(3, 1)], false));
    }

    #[test]
    fn unbalanced_markers_give_no_ranges_and_are_flagged() {
        let limits = DryMergeLimits {
            marker_size: 7,
            ..DryMergeLimits::default()
        };
        let (o, t, cut) = hunk_ranges(b"<<<<<<< o\nX\n", b"X\n", b"Y\n", &limits);
        assert!(o.is_empty() && t.is_empty() && cut);
    }

    #[test]
    fn blocks_far_apart_on_one_side_only_stay_two() {
        let blocks = vec![(r(2, 1), r(2, 1)), (r(5, 1), r(20, 1))];
        assert_eq!(join(blocks.clone(), 3), blocks);
    }
}
