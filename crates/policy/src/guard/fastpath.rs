//! The fast path of the hook layer (ADR-GRD-002 § 4, ADR-GRD-001 § 3), in plain `std`: the
//! native dispatcher `raptor-hook` compiles this same file (`#[path]`), so the classification
//! of refs is written once and the dispatcher decides without starting `raptor` or reaching the
//! daemon.
//!
//! The fast path only ever **allows**: a line it does not fully understand is not skippable and
//! goes to `raptor hook`, which validates it strictly and fails closed.

use std::path::Path;

/// Prefixes of refs that are not governed (ADR-GRD-002 § 4).
pub const NOT_GOVERNED_PREFIXES: &[&str] = &[
    "refs/remotes/",
    "refs/tags/",
    "refs/notes/",
    "refs/bisect/",
    "refs/rewritten/",
    "refs/prefetch/",
];

/// A pseudo-ref by the gitglossary(7) rule: one component, uppercase letters and underscores,
/// outside `refs/`, other than `HEAD` (E-02-6).
pub fn is_pseudo_ref(name: &str) -> bool {
    name != "HEAD" && !name.is_empty() && name.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
}

/// `HEAD` in its worktree forms: `HEAD`, `main-worktree/HEAD`, `worktrees/<id>/HEAD`.
pub fn is_head(name: &str) -> bool {
    name == "HEAD"
        || name == "main-worktree/HEAD"
        || name
            .strip_prefix("worktrees/")
            .and_then(|rest| rest.strip_suffix("/HEAD"))
            .is_some_and(|id| !id.is_empty() && !id.contains('/'))
}

/// Strips the per-worktree prefixes of a pseudo-ref or `HEAD`.
fn worktree_local(name: &str) -> &str {
    if let Some(rest) = name.strip_prefix("main-worktree/") {
        return rest;
    }
    if let Some(rest) = name.strip_prefix("worktrees/")
        && let Some((_, tail)) = rest.split_once('/')
    {
        return tail;
    }
    name
}

/// Whether an update of `name` is governed. `HEAD` itself is governed only through the branch
/// it resolves to: the hook client resolves it first, so a `HEAD` reaching the decision is
/// detached (not governed).
pub fn is_governed(name: &str) -> bool {
    if NOT_GOVERNED_PREFIXES.iter().any(|p| name.starts_with(p)) || name == "refs/stash" {
        return false;
    }
    let local = worktree_local(name);
    !(local == "HEAD" || is_pseudo_ref(local))
}

/// A value of a hook line: an object id (40 or 64 lowercase hex), all zero, or `ref:<target>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    Zero,
    Oid(&'a str),
    Symbolic,
}

fn value(field: &str, symbolic: bool) -> Option<Value<'_>> {
    if symbolic && let Some(target) = field.strip_prefix("ref:") {
        return plain_name(target).then_some(Value::Symbolic);
    }
    let hex = field
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !hex || !(field.len() == 40 || field.len() == 64) {
        return None;
    }
    Some(if field.bytes().all(|b| b == b'0') {
        Value::Zero
    } else {
        Value::Oid(field)
    })
}

/// A conservative subset of `check-ref-format`: printable ASCII without the forbidden
/// characters, no empty, dot-leading or `.lock` component, no `..` and no `@{`. Anything else is
/// not skippable here (the strict parser decides).
pub fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && !name.starts_with('-')
        && !name.contains("..")
        && !name.contains("@{")
        && name.bytes().all(|b| {
            b.is_ascii_graphic() && !matches!(b, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
        })
        && name.split('/').all(|c| {
            !c.is_empty() && !c.starts_with('.') && !c.ends_with(".lock") && !c.ends_with('.')
        })
}

/// One `reference-transaction` line split in its three fields.
pub fn split_ref_update(line: &str) -> Option<(Value<'_>, Value<'_>, &str)> {
    let mut f = line.split(' ');
    let (Some(old), Some(new), Some(name), None) = (f.next(), f.next(), f.next(), f.next()) else {
        return None;
    };
    if !plain_name(name) {
        return None;
    }
    Some((value(old, true)?, value(new, true)?, name))
}

/// One `pre-push` line: the remote ref and the local value.
pub fn split_push_update(line: &str) -> Option<(Value<'_>, &str, Value<'_>)> {
    let mut f = line.split(' ');
    let (Some(local_ref), Some(local), Some(remote_ref), Some(remote), None) =
        (f.next(), f.next(), f.next(), f.next(), f.next())
    else {
        return None;
    };
    if local_ref.is_empty() || !plain_name(remote_ref) {
        return None;
    }
    Some((value(local, false)?, remote_ref, value(remote, false)?))
}

/// The lines of a hook input, or `None` if one is not UTF-8 or is longer than `max_line`.
pub fn lines(input: &[u8], max_line: usize) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    for raw in input.split(|b| *b == b'\n') {
        if raw.is_empty() {
            continue;
        }
        if raw.len() > max_line {
            return None;
        }
        out.push(std::str::from_utf8(raw).ok()?);
    }
    Some(out)
}

/// The loose-ref prune of `pack-refs` (ADR-GRD-002 § 4, SPIKE-GRD-001 § 3.3): with the files
/// backend, `<old> 0 <ref>` is not a deletion if `old` is not zero, the loose file
/// `<common>/<ref>` holds `old` and `packed-refs` holds exactly `<old> <ref>`. An explicit
/// deletion always emits the `packed-refs` transaction first with an old value of zero.
pub fn is_prune(old: Value<'_>, new: Value<'_>, name: &str, common: &Path) -> bool {
    let (Value::Oid(old), Value::Zero) = (old, new) else {
        return false;
    };
    if !name.starts_with("refs/") || !plain_name(name) {
        return false;
    }
    let loose = common.join(name);
    match std::fs::symlink_metadata(&loose) {
        Ok(m) if m.is_file() && m.len() <= 128 => {}
        _ => return false,
    }
    let Ok(content) = std::fs::read_to_string(&loose) else {
        return false;
    };
    if content.trim_end_matches(['\n', '\r']) != old {
        return false;
    }
    let packed = common.join("packed-refs");
    match std::fs::symlink_metadata(&packed) {
        Ok(m) if m.is_file() => {}
        _ => return false,
    }
    let Ok(packed) = std::fs::read_to_string(&packed) else {
        return false;
    };
    let wanted_len = old.len() + 1 + name.len();
    packed.lines().any(|l| {
        l.len() == wanted_len
            && l.starts_with(old)
            && l.as_bytes()[old.len()] == b' '
            && &l[old.len() + 1..] == name
    })
}

/// Whether every line of a `reference-transaction` in `prepared` can be allowed without
/// `raptor`: refs that are not governed, symbolic updates of `HEAD` and prunes of `pack-refs`.
pub fn skippable_ref_transaction(input: &[u8], common: &Path, max_line: usize) -> bool {
    let Some(lines) = lines(input, max_line) else {
        return false;
    };
    lines.iter().all(|line| {
        let Some((old, new, name)) = split_ref_update(line) else {
            return false;
        };
        if is_head(name) {
            // `ref:` values move `HEAD` itself, never a branch.
            return old == Value::Symbolic || new == Value::Symbolic;
        }
        !is_governed(name) || is_prune(old, new, name, common)
    })
}

/// Whether every remote ref of a `pre-push` is not governed (tags, notes…).
pub fn skippable_push(input: &[u8], max_line: usize) -> bool {
    let Some(lines) = lines(input, max_line) else {
        return false;
    };
    lines.iter().all(|line| {
        split_push_update(line).is_some_and(|(_, remote_ref, _)| !is_governed(remote_ref))
    })
}

/// For the dispatcher without `raptor` (ADR-GRD-001 § 3): whether a `reference-transaction` in
/// `prepared` deletes a branch or a `HEAD` (that may resolve to one), other than a prune, or has
/// a line that cannot be read. Either means exit 1.
pub fn deletes_a_branch(input: &[u8], common: &Path, max_line: usize) -> bool {
    let Some(lines) = lines(input, max_line) else {
        return true;
    };
    lines.iter().any(|line| match split_ref_update(line) {
        None => true,
        Some((old, new, name)) => {
            new == Value::Zero
                && (name.starts_with("refs/heads/") || is_head(name))
                && old != Value::Symbolic
                && !is_prune(old, new, name, common)
        }
    })
}
