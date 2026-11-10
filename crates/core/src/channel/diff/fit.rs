//! The byte cut of a diff answer, shared by every `diff.*` method (DS-TS-CKP-006 D15).

use gitraptor_api::diff::index::IndexFileDiff;
use gitraptor_api::diff::{DiffCut, FileBody, FileDiff};

/// A file entry of a diff answer that can lose its content.
pub(in crate::channel) trait Fits: serde::Serialize {
    /// Drops the content: the body is omitted and the line counts go to zero.
    fn omit(&mut self);
}

impl Fits for FileDiff {
    fn omit(&mut self) {
        self.body = FileBody::Omitted {};
        (self.added, self.removed) = (0, 0);
    }
}

impl Fits for IndexFileDiff {
    fn omit(&mut self) {
        self.file.omit();
    }
}

/// Cuts `sections` so an answer of `whole` serialized bytes is at most `limit`: the first files
/// keep their content, the ones after the cut come out as omitted (and, if even that does not
/// fit, are left out of the list), and the section says `bytes`. The totals are never touched.
pub(in crate::channel) fn fit_sections<F: Fits>(
    whole: usize,
    sections: Vec<(&mut Vec<F>, &mut Option<DiffCut>)>,
    limit: usize,
) {
    if whole <= limit {
        return;
    }
    let size = |v: &F| serde_json::to_vec(v).map_or(0, |b| b.len() + 1);
    let files_size: usize = sections
        .iter()
        .flat_map(|(files, _)| files.iter())
        .map(size)
        .sum();
    let mut used = whole.saturating_sub(files_size);
    let mut over = false;
    for (files, cut) in sections {
        let mut kept = Vec::with_capacity(files.len());
        for mut file in std::mem::take(files) {
            let mut sz = size(&file);
            if !over && used + sz <= limit {
                used += sz;
                kept.push(file);
                continue;
            }
            over = true;
            *cut = Some(DiffCut::Bytes);
            file.omit();
            sz = size(&file);
            if used + sz <= limit {
                used += sz;
                kept.push(file);
            }
        }
        *files = kept;
    }
}
