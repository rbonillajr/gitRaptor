//! The ingest of `diff.worktree` (DS-US-CKP-012, T007): the only place where the untrusted paths
//! and lines of a [`WorktreeDiff`] become [`SafeText`] rows (SEC-12). The model and the view hold
//! rows, never the response.
//!
//! A tab is expanded to the next multiple of four columns and the `\r` of a CRLF is dropped
//! before sanitizing: the sanitizer escapes every control (`\x09`, `\x0d`), which would make a
//! file with tabs or Windows line ends unreadable. Every other control stays escaped.

use gitraptor_api::Untrusted;
use gitraptor_api::diff::{
    CommittedDiff, CommittedGap, DiffCut, DiffSection, FileBody, FileChange, FileDiff, LineKind,
    WorktreeDiff,
};

use super::SafeText;
use crate::tui::widgets::diff_view::{DiffLine, DiffLineKind};

/// Columns of a tab stop.
const TAB: usize = 4;

/// A notice on a file's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileNote {
    Binary,
    TooLarge,
    Symlink,
    Submodule,
    /// A limit of the section was reached before this file's content.
    Omitted,
    /// The file has more diff lines than the view lists.
    LinesCut,
}

/// Which of the two sections a header opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Committed,
    Uncommitted,
}

/// A file's header: its path, what changed and the lines added and removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHead {
    pub path: SafeText,
    pub change: FileChange,
    pub added: u32,
    pub removed: u32,
    pub note: Option<FileNote>,
}

/// One row of the scrollable view. Sentences are not here: the view gets them from the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    Section {
        kind: SectionKind,
        files: u32,
    },
    /// The branch's section cannot be computed.
    Gap(CommittedGap),
    /// The section lists fewer files than it has.
    Cut(DiffCut),
    File(FileHead),
    Line(DiffLine),
}

/// The rows of the view, in order: each section's header, its cut notice, and per file its
/// header and the lines of its hunks.
pub fn rows(diff: &WorktreeDiff) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    match &diff.committed {
        CommittedDiff::Ready { section, .. } => {
            section_rows(&mut rows, SectionKind::Committed, section)
        }
        CommittedDiff::NotComputable { reason } => rows.push(DiffRow::Gap(*reason)),
    }
    section_rows(&mut rows, SectionKind::Uncommitted, &diff.uncommitted);
    rows
}

fn section_rows(rows: &mut Vec<DiffRow>, kind: SectionKind, section: &DiffSection) {
    rows.push(DiffRow::Section {
        kind,
        files: section.files_total,
    });
    if let Some(cut) = section.cut {
        rows.push(DiffRow::Cut(cut));
    }
    for file in &section.files {
        file_rows(rows, file);
    }
}

fn file_rows(rows: &mut Vec<DiffRow>, file: &FileDiff) {
    let note = match &file.body {
        FileBody::Text { truncated, .. } => truncated.then_some(FileNote::LinesCut),
        FileBody::Binary {} => Some(FileNote::Binary),
        FileBody::TooLarge {} => Some(FileNote::TooLarge),
        FileBody::Submodule {} => Some(FileNote::Submodule),
        FileBody::Symlink {} => Some(FileNote::Symlink),
        FileBody::Omitted {} => Some(FileNote::Omitted),
    };
    rows.push(DiffRow::File(FileHead {
        path: SafeText::from_untrusted(&file.path),
        change: file.change,
        added: file.added,
        removed: file.removed,
        note,
    }));
    let FileBody::Text { hunks, .. } = &file.body else {
        return;
    };
    for hunk in hunks {
        rows.push(DiffRow::Line(DiffLine {
            kind: DiffLineKind::Hunk,
            old: None,
            new: None,
            text: SafeText::text(&format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_len, hunk.new_start, hunk.new_len
            )),
        }));
        let (mut old, mut new) = (hunk.old_start, hunk.new_start);
        for line in &hunk.lines {
            let (kind, o, n) = match line.kind {
                LineKind::Context => {
                    let at = (Some(old), Some(new));
                    old = old.saturating_add(1);
                    new = new.saturating_add(1);
                    (DiffLineKind::Context, at.0, at.1)
                }
                LineKind::Added => {
                    let at = Some(new);
                    new = new.saturating_add(1);
                    (DiffLineKind::Added, None, at)
                }
                LineKind::Removed => {
                    let at = Some(old);
                    old = old.saturating_add(1);
                    (DiffLineKind::Removed, at, None)
                }
            };
            rows.push(DiffRow::Line(DiffLine {
                kind,
                old: o,
                new: n,
                text: line_text(&line.text),
            }));
        }
    }
}

/// A line of a file, tabs expanded and a final `\r` dropped, then sanitized.
fn line_text(raw: &Untrusted) -> SafeText {
    let text = raw.raw();
    let text = text.strip_suffix('\r').unwrap_or(text);
    let mut flat = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        if c == '\t' {
            let fill = TAB - column % TAB;
            flat.extend(std::iter::repeat_n(' ', fill));
            column += fill;
        } else {
            flat.push(c);
            column += 1;
        }
    }
    SafeText::from_untrusted(&Untrusted::new(flat))
}

/// `HH:MM:SS` (UTC) of a time in milliseconds since the epoch. The standard library has no
/// local time zone, and a clock the developer cannot place would mislead, so the view says UTC.
pub fn clock(utc_ms: i64) -> String {
    let secs = utc_ms.div_euclid(1000).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use gitraptor_api::diff::{DiffLineView, Hunk};

    use super::*;

    fn text_file(path: &str, lines: &[(LineKind, &str)]) -> FileDiff {
        FileDiff {
            path: Untrusted::new(path),
            change: FileChange::Modified,
            added: 1,
            removed: 0,
            body: FileBody::Text {
                truncated: false,
                hunks: vec![Hunk {
                    old_start: 10,
                    old_len: 2,
                    new_start: 10,
                    new_len: 3,
                    lines: lines
                        .iter()
                        .map(|(kind, text)| DiffLineView {
                            kind: *kind,
                            text: Untrusted::new(*text),
                        })
                        .collect(),
                }],
            },
        }
    }

    fn diff_of(file: FileDiff) -> WorktreeDiff {
        WorktreeDiff {
            computed_at_utc_ms: 0,
            committed: CommittedDiff::NotComputable {
                reason: CommittedGap::NoBase,
            },
            uncommitted: DiffSection {
                files_total: 1,
                files: vec![file],
                cut: None,
            },
        }
    }

    fn painted(rows: &[DiffRow]) -> Vec<String> {
        rows.iter()
            .filter_map(|r| match r {
                DiffRow::Line(l) if l.kind != DiffLineKind::Hunk => Some(l.text.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tabs_expand_and_crlf_is_trimmed() {
        let file = text_file(
            "a.txt",
            &[
                (LineKind::Context, "col1\tcol2\r"),
                (LineKind::Added, "\tx"),
                (LineKind::Added, "ab\tc"),
                (LineKind::Removed, "end\r\r"),
            ],
        );
        let rows = rows(&diff_of(file));
        assert_eq!(
            painted(&rows),
            ["col1    col2", "    x", "ab  c", "end\\x0d"]
        );
        // The numbers come from the hunk: context moves both, added the new, removed the old.
        let numbers: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::Line(l) if l.kind != DiffLineKind::Hunk => Some((l.old, l.new)),
                _ => None,
            })
            .collect();
        assert_eq!(
            numbers,
            [
                (Some(10), Some(10)),
                (None, Some(11)),
                (None, Some(12)),
                (Some(11), None)
            ]
        );
    }

    #[test]
    fn lines_with_controls_are_sanitized() {
        let evil = "evil\u{1b}]52;c;aGk=\u{7}\u{202e}gpj\u{2028}.txt";
        let file = text_file(
            evil,
            &[
                (LineKind::Added, "\u{1b}]52;c;aGk=\u{7}"),
                (LineKind::Added, "rlo \u{202e}abc"),
                (LineKind::Added, "sep\u{2028}end"),
                (LineKind::Added, "esc \u{1b}[2Jgone"),
            ],
        );
        let rows = rows(&diff_of(file));
        let mut every: Vec<String> = painted(&rows);
        every.extend(rows.iter().filter_map(|r| match r {
            DiffRow::File(f) => Some(f.path.to_string()),
            _ => None,
        }));
        assert_eq!(every.len(), 5);
        for text in &every {
            assert!(
                !text
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '\u{202e}' | '\u{2028}' | '\u{2029}')),
                "raw control in {text:?}"
            );
        }
        // The sequence shows as text, it is not obeyed.
        assert!(every[0].contains("52;c;aGk="), "{every:?}");
    }

    #[test]
    fn notices_and_gaps_become_rows() {
        let mut file = text_file("p.bin", &[]);
        file.body = FileBody::Binary {};
        let rows = rows(&diff_of(file));
        assert!(matches!(rows[0], DiffRow::Gap(CommittedGap::NoBase)));
        assert!(matches!(
            rows[1],
            DiffRow::Section {
                kind: SectionKind::Uncommitted,
                files: 1
            }
        ));
        assert!(
            matches!(&rows[2], DiffRow::File(f) if f.note == Some(FileNote::Binary)),
            "{rows:?}"
        );
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn the_clock_is_hours_minutes_seconds() {
        assert_eq!(clock(1_793_091_792_000), "09:03:12");
        assert_eq!(clock(-1000), "23:59:59");
    }
}
