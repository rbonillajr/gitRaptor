//! Conflict line ranges from a merged blob with markers (F6), carried to the coordinates of
//! each side's version (ADR-CKP-001 § 3): the merged blob does not give them directly, because
//! the lines around a conflict may come from either side.
//!
//! Method: rebuild the "ours view" (every conflict block replaced by its ours lines) and map it
//! to the ours blob with a line diff; the ours lines of a block are verbatim from ours, so the
//! diff keeps them as unchanged lines. Same for theirs. The same function runs on Git's merged
//! blob, so the comparison of E7 is about the merge, not the parser.

use gix_diff::blob::{Algorithm, Diff, InternedInput};

fn lines(data: &[u8]) -> Vec<&[u8]> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<&[u8]> = data.split_inclusive(|b| *b == b'\n').collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

/// A marker line: exactly `size` times `ch`, then a space, a newline or the end.
fn is_marker(line: &[u8], ch: u8, size: usize) -> bool {
    line.len() >= size
        && line[..size].iter().all(|b| *b == ch)
        && matches!(line.get(size), None | Some(b' ' | b'\n' | b'\r'))
}

#[derive(Debug, Default)]
struct Block {
    ours: Vec<usize>,   // indices into the ours view
    theirs: Vec<usize>, // indices into the theirs view
    ours_at: usize,     // insertion position in the ours view
    theirs_at: usize,
}

/// Returns `None` when the markers are unbalanced (a parse failure is reported, never guessed).
pub fn side_ranges(merged: &[u8], ours: &[u8], theirs: &[u8], marker: u8) -> Option<Vec<[u32; 4]>> {
    let size = marker as usize;
    let mut ours_view: Vec<&[u8]> = Vec::new();
    let mut theirs_view: Vec<&[u8]> = Vec::new();
    let mut blocks = Vec::new();
    let mut state = 0u8; // 0 outside, 1 ours part, 2 base part (diff3), 3 theirs part
    let mut cur = Block::default();
    for line in lines(merged) {
        match state {
            0 if is_marker(line, b'<', size) => {
                state = 1;
                cur = Block {
                    ours_at: ours_view.len(),
                    theirs_at: theirs_view.len(),
                    ..Default::default()
                };
            }
            0 => {
                ours_view.push(line);
                theirs_view.push(line);
            }
            1 if is_marker(line, b'|', size) => state = 2,
            1 | 2 if is_marker(line, b'=', size) => state = 3,
            1 => {
                cur.ours.push(ours_view.len());
                ours_view.push(line);
            }
            2 => {}
            3 if is_marker(line, b'>', size) => {
                state = 0;
                blocks.push(std::mem::take(&mut cur));
            }
            3 => {
                cur.theirs.push(theirs_view.len());
                theirs_view.push(line);
            }
            _ => return None,
        }
    }
    if state != 0 {
        return None;
    }
    let om = view_to_side(&ours_view.concat(), ours);
    let tm = view_to_side(&theirs_view.concat(), theirs);
    let range = |idx: &[usize], at: usize, map: &[Option<u32>]| -> Option<[u32; 2]> {
        if idx.is_empty() {
            // Nothing on this side: an empty range just after the mapped previous line.
            let prev = if at == 0 {
                Some(0)
            } else {
                map.get(at - 1).copied().flatten().map(|l| l + 1)
            };
            return prev.map(|p| [p + 1, p]);
        }
        let first = map.get(idx[0]).copied().flatten()?;
        let last = map.get(*idx.last()?).copied().flatten()?;
        Some([first + 1, last + 1])
    };
    let mut out = Vec::new();
    for b in &blocks {
        let o = range(&b.ours, b.ours_at, &om)?;
        let t = range(&b.theirs, b.theirs_at, &tm)?;
        out.push([o[0], o[1], t[0], t[1]]);
    }
    Some(out)
}

/// For every line of `view`, its 0-based line in `side` when the diff keeps it unchanged.
fn view_to_side(view: &[u8], side: &[u8]) -> Vec<Option<u32>> {
    let n = lines(view).len();
    let input = InternedInput::new(side, view);
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let mut map = vec![None; n];
    let (mut s, mut v) = (0u32, 0u32);
    for h in diff.hunks() {
        while v < h.after.start {
            map[v as usize] = Some(s);
            s += 1;
            v += 1;
        }
        s = h.before.end;
        v = h.after.end;
    }
    while (v as usize) < n {
        map[v as usize] = Some(s);
        s += 1;
        v += 1;
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_block_to_each_side() {
        let ours = b"a\nb\nX\nd\n";
        let theirs = b"a\nY1\nY2\nd\ne\n";
        let m = b"a\nb\n<<<<<<< ours\nX\n=======\nY1\nY2\n>>>>>>> theirs\nd\ne\n";
        // Ours keeps `b` (theirs dropped it? no: block covers X vs Y1,Y2); `b` is outside.
        let r = side_ranges(m, ours, theirs, 7).unwrap();
        assert_eq!(r, vec![[3, 3, 2, 3]]);
    }

    #[test]
    fn a_line_that_imitates_a_marker_is_not_a_marker_with_a_bigger_size() {
        let line = b"<<<<<<< not a marker\n";
        assert!(is_marker(line, b'<', 7));
        assert!(!is_marker(line, b'<', 32));
    }
}
