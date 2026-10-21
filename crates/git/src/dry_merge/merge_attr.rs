//! The mark of a merge rule of the user's own: does the tree of either tip give a file the
//! `merge` attribute (`merge=<x>`, `merge` or `-merge`)?
//!
//! The `.gitattributes` files are read from the trees of the tips only (the file's directory and
//! its parents), as blobs through the job's bounded object access, and matched with a search of
//! their own. Never the working tree, the index, `info/attributes` or `core.attributesFile`; and
//! this search never reaches the merge, which runs with an empty attribute stack.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gix::attrs::search::{MetadataCollection, Outcome};
use gix::attrs::{Search, StateRef};
use gix::bstr::ByteSlice;
use gix::objs::{FindExt, FindObjectOrHeader};

const ATTRIBUTES_FILE: &str = ".gitattributes";

/// Looks up the `merge` attribute in the trees of the tips, caching each `.gitattributes`.
pub(crate) struct MergeRules<'a, O> {
    objects: &'a O,
    trees: Vec<gix::ObjectId>,
    max_blob_bytes: u64,
    /// `(tip, directory)` → the bytes of its `.gitattributes`, if it has one.
    files: HashMap<(usize, String), Option<Vec<u8>>>,
}

impl<'a, O: FindObjectOrHeader> MergeRules<'a, O> {
    /// `trees`: the root trees of the tips. A `.gitattributes` above `max_blob_bytes` (by its
    /// header) is not read.
    pub(crate) fn new(objects: &'a O, trees: Vec<gix::ObjectId>, max_blob_bytes: u64) -> Self {
        Self {
            objects,
            trees,
            max_blob_bytes,
            files: HashMap::new(),
        }
    }

    /// Whether the tree of any tip sets, unsets or gives a value to `merge` for `path`.
    pub(crate) fn has_merge_rule(&mut self, path: &str) -> gix::Result<bool> {
        for tip in 0..self.trees.len() {
            if self.tip_has_rule(tip, path)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn tip_has_rule(&mut self, tip: usize, path: &str) -> gix::Result<bool> {
        let mut collection = MetadataCollection::default();
        // Only the built-in `binary` macro (`-diff -merge -text`): no file is read.
        let mut search = Search::new_globals(
            std::iter::empty::<PathBuf>(),
            &mut Vec::new(),
            &mut collection,
        )
        .map_err(gix::Error::from_error)?;
        let mut any = false;
        for dir in directories_of(path) {
            let Some(bytes) = self.attributes_file(tip, &dir)? else {
                continue;
            };
            any = true;
            let source = Path::new(&dir).join(ATTRIBUTES_FILE);
            // Git honours macro definitions only at the top level.
            search
                .add_patterns_buffer(
                    &bytes,
                    source,
                    Some(Path::new("")),
                    &mut collection,
                    dir.is_empty(),
                )
                .map_err(gix::Error::from_error)?;
        }
        if !any {
            return Ok(false);
        }
        let mut out = Outcome::default();
        out.initialize_with_selection(&collection, ["merge"]);
        search.pattern_matching_relative_path(
            path.as_bytes().as_bstr(),
            gix::glob::pattern::Case::Sensitive,
            Some(false),
            &mut out,
        );
        Ok(out
            .iter_selected()
            .any(|m| !matches!(m.assignment.state, StateRef::Unspecified)))
    }

    /// The `.gitattributes` of `dir` (`""` for the root) in the tree of `tip`.
    fn attributes_file(&mut self, tip: usize, dir: &str) -> gix::Result<Option<Vec<u8>>> {
        let key = (tip, dir.to_owned());
        if let Some(cached) = self.files.get(&key) {
            return Ok(cached.clone());
        }
        let found = self.read_attributes_file(self.trees[tip], dir)?;
        self.files.insert(key, found.clone());
        Ok(found)
    }

    fn read_attributes_file(&self, root: gix::ObjectId, dir: &str) -> gix::Result<Option<Vec<u8>>> {
        let mut buf = Vec::new();
        let mut tree = root;
        for name in dir.split('/').filter(|n| !n.is_empty()) {
            let Some(next) = self.entry(tree, name, &mut buf, |m| m.is_tree())? else {
                return Ok(None);
            };
            tree = next;
        }
        let Some(blob) = self.entry(tree, ATTRIBUTES_FILE, &mut buf, |m| m.is_blob())? else {
            return Ok(None);
        };
        match self.objects.try_header(&blob)? {
            Some(h) if h.size <= self.max_blob_bytes => {}
            _ => return Ok(None),
        }
        Ok(Some(self.objects.find_blob(&blob, &mut buf)?.data.to_vec()))
    }

    fn entry(
        &self,
        tree: gix::ObjectId,
        name: &str,
        buf: &mut Vec<u8>,
        mode: impl Fn(gix::objs::tree::EntryMode) -> bool,
    ) -> gix::Result<Option<gix::ObjectId>> {
        let tree = self.objects.find_tree(&tree, buf)?;
        Ok(tree
            .entries
            .iter()
            .find(|e| e.filename == name.as_bytes() && mode(e.mode))
            .map(|e| e.oid.to_owned()))
    }
}

/// The directories whose `.gitattributes` apply to `path`, root (`""`) first.
fn directories_of(path: &str) -> Vec<String> {
    let mut dirs = vec![String::new()];
    let mut end = 0;
    let parts: Vec<&str> = path.split('/').collect();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        end += part.len();
        dirs.push(path[..end].to_owned());
        end += 1;
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directories_of_a_path_go_from_the_root_down() {
        assert_eq!(directories_of("a.txt"), vec![String::new()]);
        assert_eq!(
            directories_of("src/deep/c.txt"),
            vec![String::new(), "src".to_owned(), "src/deep".to_owned()]
        );
    }
}
