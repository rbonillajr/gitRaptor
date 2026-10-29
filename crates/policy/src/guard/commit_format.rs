//! The format of a commit message (US-GRD-009, DS-US-GRD-009 D8 and D9): Conventional Commits
//! 1.0.0 over the first line, read as bytes, in one pass and with no regular expression, so the
//! cost is linear in a bounded input (ADR-GRD-003 § 1). Pure: the caller reads the object and
//! hands over only the first line and two facts about the rest.

/// Longest first line that can conform, in bytes.
pub const MAX_HEADER_BYTES: usize = 4_096;
/// Longest scope, in bytes.
const MAX_SCOPE_BYTES: usize = 64;
/// Prefixes of `--fixup`, `--squash` and `--fixup=amend:` removed before the grammar.
const MAX_PREFIXES: usize = 8;
const PREFIXES: [&[u8]; 3] = [b"fixup! ", b"squash! ", b"amend! "];

/// What the grammar says of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// The first line follows the format (or is a message Git writes) and a body is separated.
    Conforms,
    /// The first line does not follow the format.
    NotConventional,
    /// The first line follows the format but a body is glued to it.
    BodyNotSeparated,
}

/// The facts of one message the rules need; the message itself is gone.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Header {
    pub verdict: Verdict,
    /// The type in lowercase, to compare with the list of the repo; `None` for a message that
    /// is not in the format or that Git writes.
    pub kind: Option<String>,
}

/// Judges a message from its first line (`head`, without its line end), whether its second line
/// is blank or absent (`body_separated`) and the number of parents of its commit.
pub fn parse(head: &[u8], body_separated: bool, parents: usize) -> Header {
    parse_with(head, body_separated, parents, false)
}

/// Like [`parse`], with `reverts`: the body carries the `This reverts commit <oid>.` line that
/// `git revert` writes.
pub fn parse_with(head: &[u8], body_separated: bool, parents: usize, reverts: bool) -> Header {
    if git_written(head, parents, reverts) {
        return Header {
            verdict: Verdict::Conforms,
            kind: None,
        };
    }
    if head.len() > MAX_HEADER_BYTES {
        return not_conventional();
    }
    let mut rest = head;
    let mut prefixed = false;
    for _ in 0..MAX_PREFIXES {
        match PREFIXES.iter().find_map(|p| rest.strip_prefix(*p)) {
            Some(after) => {
                rest = after;
                prefixed = true;
            }
            None => break,
        }
    }
    // A fixup of a revert Git wrote: the commit it targets was exempt, and its own body does not
    // carry the revert line.
    if prefixed && (rest.starts_with(b"Revert \"") || rest.starts_with(b"Reapply \"")) {
        return Header {
            verdict: Verdict::Conforms,
            kind: None,
        };
    }
    let Some(kind) = grammar(rest) else {
        return not_conventional();
    };
    Header {
        verdict: if body_separated {
            Verdict::Conforms
        } else {
            Verdict::BodyNotSeparated
        },
        kind: Some(kind),
    }
}

/// A message Git writes, which does not break the format (D9): the first line of a merge commit
/// (`Merge …`, two or more parents) and `Revert "…"` or `Reapply "…"` over the line `git revert`
/// writes in the body. An agent can imitate them: the format is a matter of readability.
pub fn git_written(head: &[u8], parents: usize, reverts: bool) -> bool {
    (parents >= 2 && head.starts_with(b"Merge "))
        || (reverts && (head.starts_with(b"Revert \"") || head.starts_with(b"Reapply \"")))
}

fn not_conventional() -> Header {
    Header {
        verdict: Verdict::NotConventional,
        kind: None,
    }
}

/// `type(scope)!: description`; the type in lowercase when the line follows it.
fn grammar(line: &[u8]) -> Option<String> {
    let first = *line.first()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let type_len = line
        .iter()
        .position(|b| !(b.is_ascii_alphanumeric() || *b == b'-'))
        .unwrap_or(line.len());
    let kind = String::from_utf8_lossy(&line[..type_len]).to_ascii_lowercase();
    let mut rest = &line[type_len..];
    if let Some(after) = rest.strip_prefix(b"(") {
        let close = after
            .iter()
            .position(|b| matches!(b, b')' | b'(' | b'\n'))?;
        if after[close] != b')' || close == 0 || close > MAX_SCOPE_BYTES {
            return None;
        }
        rest = &after[close + 1..];
    }
    rest = rest.strip_prefix(b"!").unwrap_or(rest);
    let description = rest.strip_prefix(b": ")?;
    description
        .iter()
        .any(|b| !b.is_ascii_whitespace())
        .then_some(kind)
}
