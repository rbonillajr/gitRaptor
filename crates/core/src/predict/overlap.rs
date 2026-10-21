//! The overlap of a pair: the intersection of the paths both sides touch. Reads no content.

use std::collections::BTreeSet;

use super::Overlap;

/// The paths one side of a pair touches: its own work, or what the base did since the fork.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnWork {
    pub paths: BTreeSet<String>,
    /// The set was cut: there may be more paths than these.
    pub partial: bool,
}

impl OwnWork {
    /// Nothing known and nothing cut: this side forms no pairs.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.partial
    }
}

/// The paths in both, sorted; partial when either input is.
pub fn intersect(a: &OwnWork, b: &OwnWork) -> Overlap {
    Overlap {
        files: a.paths.intersection(&b.paths).cloned().collect(),
        partial: a.partial || b.partial,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own(paths: &[&str], partial: bool) -> OwnWork {
        OwnWork {
            paths: paths.iter().map(|s| (*s).to_owned()).collect(),
            partial,
        }
    }

    #[test]
    fn the_intersection_is_sorted_and_partial_follows_either_side() {
        let o = intersect(&own(&["b", "a"], false), &own(&["a", "b", "c"], true));
        assert_eq!(o.files, ["a", "b"]);
        assert!(o.partial);
    }

    #[test]
    fn a_cut_empty_side_is_not_empty_work() {
        assert!(own(&[], false).is_empty());
        assert!(!own(&[], true).is_empty());
    }
}
