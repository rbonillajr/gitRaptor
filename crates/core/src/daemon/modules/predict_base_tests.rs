//! `Msg::BaseChanged` of the predictor's worker (TS-CKP-011). In its own file: the boundary
//! test of the predictor (`crates/core/tests/predict_boundary.rs`) reads `predict.rs` whole, and
//! the temporary profile these tests need is a layer the shipped predictor must not name.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use super::*;

/// A worker without a pool over a temporary profile (NFR-01): nothing is merged nor
/// published.
fn worker(tmp: &Path) -> Worker {
    let dirs = crate::profile::ProfileDirs::under_root(tmp.join("profile"));
    let logger = Logger::open(tmp, crate::daemon::LogLimits::default()).expect("a temporary log");
    let bus = Arc::new(EventBus::new(
        "run-1".into(),
        crate::channel::EngineShared {
            engine: gitraptor_api::messages::EngineView {
                state: gitraptor_api::messages::EngineStateView::NoRepos,
                git_version: None,
            },
            repos: Vec::new(),
            divergence: BTreeMap::new(),
        },
        16,
    ));
    Worker {
        tm: Arc::new(TmRepos::new(dirs)),
        logger,
        bus,
        store: Arc::new(PredictionStore::new(false)),
        pool: None,
        saving: false,
        repos: HashMap::new(),
        deferred: BTreeMap::new(),
    }
}

#[test]
fn base_changed_leaves_a_dormant_repo_alone() {
    let tmp = tempfile::tempdir().expect("a temporary dir");
    let mut w = worker(tmp.path());
    w.state("r").dormant = true;
    let (mut dirty, mut touched) = (BTreeMap::new(), BTreeSet::new());
    w.take(Msg::BaseChanged("r".into()), &mut dirty, &mut touched);
    assert!(dirty.is_empty(), "{dirty:?}");
    assert!(touched.is_empty(), "{touched:?}");
    assert!(w.repos.get("r").is_some_and(|s| s.dormant));

    w.take(Msg::BaseChanged("s".into()), &mut dirty, &mut touched);
    assert_eq!(dirty.keys().collect::<Vec<_>>(), ["s"]);
    assert!(dirty["s"].is_empty());
}
