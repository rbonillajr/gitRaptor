//! H6 prefilter: if the committed files of the two sides (since their merge-base) do not meet,
//! neither as the same path nor as a file of one side under a directory of the other, the pair
//! cannot conflict. Rename sources are
//! deletions, so they are already in the set. Plain tree diff, no rename tracking, no blobs.

use std::collections::BTreeSet;

use gix::ObjectId;
use gix::bstr::{BString, ByteSlice};

type Paths = (BTreeSet<BString>, BTreeSet<BString>);

/// Changed files of a side, and the parent directories of those files.
fn changed(repo: &gix::Repository, base: ObjectId, side: ObjectId) -> Result<Paths, String> {
    let (mut b1, mut b2) = (Vec::new(), Vec::new());
    let t_base = repo
        .find_commit(base)
        .map_err(|e| e.to_string())?
        .tree_id()
        .map_err(|e| e.to_string())?;
    let t_side = repo
        .find_commit(side)
        .map_err(|e| e.to_string())?
        .tree_id()
        .map_err(|e| e.to_string())?;
    let a = gix_object::Find::try_find(repo, &t_base, &mut b1)
        .map_err(|e| e.to_string())?
        .ok_or("tree")?;
    let a = gix_object::TreeRefIter::from_bytes(a.data, gix_hash::Kind::Sha1);
    let b = gix_object::Find::try_find(repo, &t_side, &mut b2)
        .map_err(|e| e.to_string())?
        .ok_or("tree")?;
    let b = gix_object::TreeRefIter::from_bytes(b.data, gix_hash::Kind::Sha1);
    let mut rec = gix_diff::tree::Recorder::default();
    gix_diff::tree(a, b, &mut gix_diff::tree::State::default(), repo, &mut rec)
        .map_err(|e| e.to_string())?;
    let (mut files, mut dirs) = (BTreeSet::new(), BTreeSet::new());
    for c in rec.records {
        use gix_diff::tree::recorder::Change;
        let (p, tree) = match c {
            Change::Addition {
                path, entry_mode, ..
            }
            | Change::Deletion {
                path, entry_mode, ..
            } => (path, entry_mode.is_tree()),
            Change::Modification {
                path,
                entry_mode,
                previous_entry_mode,
                ..
            } => (path, entry_mode.is_tree() && previous_entry_mode.is_tree()),
        };
        if tree {
            continue;
        }
        let mut cur = p.as_bstr();
        files.insert(cur.to_owned());
        while let Some(i) = cur.rfind_byte(b'/') {
            cur = cur[..i].as_bstr();
            dirs.insert(cur.to_owned());
        }
    }
    Ok((files, dirs))
}

/// `Some(true)` when the two sides touch a common path (a conflict is possible).
pub fn may_conflict(
    repo: &gix::Repository,
    ours: ObjectId,
    theirs: ObjectId,
) -> Result<bool, String> {
    let base = repo.merge_base(ours, theirs).map_err(|e| e.to_string())?;
    #[cfg(not(feature = "gix088"))]
    let base = base.ok_or("no merge-base")?;
    let base = base.detach();
    let a = changed(repo, base, ours)?;
    let b = changed(repo, base, theirs)?;
    // Same file on both sides, or a file of one side where the other has a directory.
    Ok(a.0.iter().any(|p| b.0.contains(p) || b.1.contains(p))
        || b.0.iter().any(|p| a.1.contains(p)))
}
