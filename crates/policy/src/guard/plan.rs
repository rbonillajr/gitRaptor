//! What a governed plan of the executor is, as Guardrails evaluates it: the operations the hooks
//! would see, the facts preloaded for them, and the transitions each child `git` of the plan is
//! expected to make. Pure: no I/O.

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{AuthorshipFacts, CommitStage, Operation, RefUpdate, RefValue};
use serde::Serialize;

use super::config::RootMatcher;
use super::glob::Budget;
use super::policies::Touched;

/// The shape of a governed plan. Every ref name is a full `refs/heads/…` name. Serialized into
/// the plan's fingerprint; it never carries the commit message, only its facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum PlanShape {
    Merge {
        base: String,
        old: String,
        integrated: String,
    },
    Rebase {
        branch: String,
        upstream: String,
        old_tip: String,
        onto: String,
    },
    /// A new branch at `at`.
    CreateWorktree {
        branch: String,
        at: String,
    },
    DeleteWorktree {
        branch: String,
        tip: String,
    },
    Commit {
        branch: String,
        old: String,
        paths: Vec<String>,
        authorship: AuthorshipFacts,
    },
}

impl PlanShape {
    /// The operations of the catalog (BR-VAL-002) the plan certainly performs, read from its
    /// shape and never from a process chain.
    ///
    /// Deleting a worktree also deletes its branch, so it carries both permissions: with only
    /// `WorktreeRemove`, a team denying `branch-delete` would lose the branch with no denial.
    pub fn catalog_operations(&self) -> &'static [crate::settings::model::Operation] {
        use crate::settings::model::Operation as Op;
        match self {
            Self::Merge { .. } => &[Op::Merge],
            Self::Rebase { .. } => &[Op::Rebase],
            Self::CreateWorktree { .. } => &[Op::WorktreeAdd],
            Self::DeleteWorktree { .. } => &[Op::WorktreeRemove, Op::BranchDelete],
            Self::Commit { .. } => &[Op::Commit],
        }
    }
}

/// A transition a child `git` of the plan is expected to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    Exact {
        refname: String,
        old: RefValue,
        new: RefValue,
    },
    /// The new value is `integrated`, or a merge whose parents are `[old, integrated]`.
    MergeInto {
        refname: String,
        old: String,
        integrated: String,
    },
    /// `pre-rebase`.
    RebaseStart { upstream: String, branch: String },
    /// The new value descends from `onto`.
    RebaseOf {
        refname: String,
        old: String,
        onto: String,
    },
    /// `pre-commit`, `commit-msg` and the second line.
    CommitStages { authorship: AuthorshipFacts },
    /// The parent is `old` and the paths are a subset of the prepared ones.
    NewCommitOn {
        refname: String,
        old: String,
        paths: Vec<String>,
    },
    /// The abort of the atomic rebase.
    RestoreTo { refname: String, value: String },
}

/// A plan as Guardrails evaluates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub evaluated: Vec<Operation>,
    /// Aligned with `evaluated`.
    pub preset: Vec<Preset>,
    pub registered: Vec<Expected>,
}

/// Facts preloaded for one evaluated operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preset {
    pub touched: Option<Touched>,
    pub config_touched: Option<Touched>,
}

/// What the I/O side read to match a child's operation against its registered transitions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchFacts {
    pub parents: Vec<String>,
    pub descends_from_onto: bool,
    pub new_paths: Option<Vec<String>>,
}

/// Prepared paths of a commit beyond which nothing can be said (ADR-GRD-003, US-GRD-008).
pub const MAX_PREPARED_PATHS: usize = 100_000;

fn tx(refname: &str, old: RefValue, new: RefValue) -> Operation {
    Operation::RefTransaction {
        updates: vec![RefUpdate {
            refname: refname.to_owned(),
            old,
            new,
        }],
        orphan_head: None,
    }
}

fn oid(value: &str) -> RefValue {
    RefValue::Oid(value.to_owned())
}

/// The prepared paths of a commit as the forbidden paths read them, and those that are the
/// Guardrails configuration (same matcher, same folding as `protect_config`).
fn prepared(paths: &[String]) -> Preset {
    let unverifiable = || Touched {
        paths: Vec::new(),
        unverifiable: true,
    };
    if paths.len() > MAX_PREPARED_PATHS {
        return Preset {
            touched: Some(unverifiable()),
            config_touched: Some(unverifiable()),
        };
    }
    let touched = Touched {
        paths: paths.to_vec(),
        unverifiable: false,
    };
    let Some(matcher) = RootMatcher::new() else {
        return Preset {
            touched: Some(touched),
            config_touched: Some(unverifiable()),
        };
    };
    let mut budget = Budget::default();
    let mut config = Vec::new();
    for path in paths {
        let root = path.split('/').next().unwrap_or_default();
        match matcher.is_config(root, &mut budget) {
            Some(true) => config.push(path.clone()),
            Some(false) => {}
            // Out of budget: nothing can be said about the configuration.
            None => {
                return Preset {
                    touched: Some(touched),
                    config_touched: Some(unverifiable()),
                };
            }
        }
    }
    Preset {
        touched: Some(touched),
        config_touched: Some(Touched {
            paths: config,
            unverifiable: false,
        }),
    }
}

/// The operations, preloaded facts and expected transitions of `shape`. `registered` is never
/// empty, and `preset` is aligned with `evaluated`.
pub fn normalize(shape: &PlanShape) -> Normalized {
    match shape {
        PlanShape::Merge {
            base,
            old,
            integrated,
        } => Normalized {
            evaluated: vec![tx(base, oid(old), oid(integrated))],
            preset: vec![Preset::default()],
            registered: vec![Expected::MergeInto {
                refname: base.clone(),
                old: old.clone(),
                integrated: integrated.clone(),
            }],
        },
        PlanShape::Rebase {
            branch,
            upstream,
            old_tip,
            onto,
        } => Normalized {
            evaluated: vec![
                Operation::Rebase {
                    upstream: Some(Untrusted::new(upstream.as_str())),
                    branch: Some(Untrusted::new(branch.as_str())),
                },
                tx(branch, oid(onto), oid(old_tip)),
            ],
            // The paths of the rebased commits are read by the I/O side (`Hide::OldOnly`).
            preset: vec![Preset::default(), Preset::default()],
            registered: vec![
                Expected::RebaseStart {
                    upstream: upstream.clone(),
                    branch: branch.clone(),
                },
                Expected::RebaseOf {
                    refname: branch.clone(),
                    old: old_tip.clone(),
                    onto: onto.clone(),
                },
            ],
        },
        PlanShape::CreateWorktree { branch, at } => Normalized {
            evaluated: vec![tx(branch, RefValue::Zero, oid(at))],
            preset: vec![Preset::default()],
            registered: vec![Expected::Exact {
                refname: branch.clone(),
                old: RefValue::Zero,
                new: oid(at),
            }],
        },
        PlanShape::DeleteWorktree { branch, tip } => Normalized {
            evaluated: vec![tx(branch, oid(tip), RefValue::Zero)],
            preset: vec![Preset::default()],
            registered: vec![Expected::Exact {
                refname: branch.clone(),
                old: oid(tip),
                new: RefValue::Zero,
            }],
        },
        PlanShape::Commit {
            branch,
            old,
            paths,
            authorship,
        } => Normalized {
            // The new oid does not exist yet: the branch is evaluated with the sentinel
            // `old -> old` (the protected branch acts on the name, the paths are preloaded).
            // `pre-commit` goes last: the gate's log entry takes the first evaluated operation,
            // which must stay `commit-msg`.
            evaluated: vec![
                Operation::Commit {
                    stage: CommitStage::CommitMsg,
                },
                tx(branch, oid(old), oid(old)),
                Operation::Commit {
                    stage: CommitStage::PreCommit,
                },
            ],
            preset: vec![Preset::default(), prepared(paths), Preset::default()],
            registered: vec![
                Expected::CommitStages {
                    authorship: authorship.clone(),
                },
                Expected::NewCommitOn {
                    refname: branch.clone(),
                    old: old.clone(),
                    paths: paths.clone(),
                },
            ],
        },
    }
}

fn is_oid(value: &RefValue, expected: &str) -> bool {
    matches!(value, RefValue::Oid(v) if v == expected)
}

/// Whether one update of a transaction is one of the registered transitions.
fn update_matches(registered: &[Expected], update: &RefUpdate, facts: &MatchFacts) -> bool {
    registered.iter().any(|expected| match expected {
        Expected::Exact { refname, old, new } => {
            update.refname == *refname && update.old == *old && update.new == *new
        }
        Expected::MergeInto {
            refname,
            old,
            integrated,
        } => {
            update.refname == *refname
                && is_oid(&update.old, old)
                && matches!(update.new, RefValue::Oid(_))
                && (is_oid(&update.new, integrated)
                    || facts.parents.len() == 2
                        && facts.parents[0] == *old
                        && facts.parents[1] == *integrated)
        }
        Expected::RebaseOf { refname, old, .. } => {
            update.refname == *refname
                && is_oid(&update.old, old)
                && matches!(update.new, RefValue::Oid(_))
                && facts.descends_from_onto
        }
        Expected::NewCommitOn {
            refname,
            old,
            paths,
        } => {
            update.refname == *refname
                && is_oid(&update.old, old)
                && matches!(update.new, RefValue::Oid(_))
                && facts.parents.len() == 1
                && facts.parents[0] == *old
                && facts
                    .new_paths
                    .as_ref()
                    .is_some_and(|new| new.iter().all(|p| paths.contains(p)))
        }
        Expected::RestoreTo { refname, value } => {
            update.refname == *refname && is_oid(&update.new, value)
        }
        Expected::RebaseStart { .. } | Expected::CommitStages { .. } => false,
    })
}

/// Whether `op` is one of the `registered` transitions. Strict: every update of a transaction
/// must be one, and in doubt it does not match (the caller evaluates again).
pub fn matches(
    registered: &[Expected],
    op: &Operation,
    authorship: Option<&AuthorshipFacts>,
    facts: &MatchFacts,
) -> bool {
    match op {
        Operation::RefTransaction { updates, .. } => {
            !updates.is_empty() && updates.iter().all(|u| update_matches(registered, u, facts))
        }
        Operation::Rebase { upstream, branch } => {
            let (Some(up), Some(br)) = (upstream, branch) else {
                return false;
            };
            registered.iter().any(|e| {
                matches!(e, Expected::RebaseStart { upstream, branch }
                    if up.raw() == upstream && br.raw() == branch)
            })
        }
        Operation::Commit { stage } => registered.iter().any(|e| match e {
            Expected::CommitStages { authorship: plan } => match stage {
                CommitStage::PreCommit => true,
                CommitStage::CommitMsg | CommitStage::SecondLine => authorship == Some(plan),
            },
            _ => false,
        }),
        Operation::Push { .. } => false,
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "plan_commit_tests.rs"]
mod commit_tests;
