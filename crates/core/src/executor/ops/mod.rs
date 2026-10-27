//! The production catalog of user operations: one arm and one file per operation, in the order
//! of [`OperationId`]. Only the arms that exist are served; every other operation answers "not
//! implemented" with its story.

mod commit;
mod commit_paths;
mod snapshot;
#[cfg(debug_assertions)]
mod test_arms;

use std::sync::Arc;

pub use commit::CommitEnv;

use gitraptor_api::catalog::{OperationArgs, OperationId, entry};

#[cfg(test)]
use super::NoGuardrails;
use super::{GuardrailsGate, OpPlan, PlanError, RepoFacts, StepPlan};
use crate::timemachine::protected::{
    DEFAULT_PRIOR_DEADLINE, OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle,
    StepError,
};

/// The operations this build serves.
const ARMS: &[OperationId] = &[OperationId::Commit, OperationId::Snapshot];

/// Dispatches by operation; holds only what the daemon read once at start.
#[derive(Clone)]
pub struct ProductionCatalog {
    armed: Vec<OperationId>,
    /// The system Git and `HOME` of `commit`; `None`: `commit` is `git-unavailable`.
    commit_env: Option<CommitEnv>,
    /// The debug test arms, when the daemon was started for them (D14).
    #[cfg(debug_assertions)]
    test_arms: Option<test_arms::TestArms>,
}

/// The arms that may be served with a gate that never allows: a governed operation needs a
/// real Guardrails gate, so its arm is left out (fail closed).
fn armed_without_guardrails(arms: &[OperationId]) -> Vec<OperationId> {
    arms.iter()
        .copied()
        .filter(|op| entry(*op).governed.is_none())
        .collect()
}

impl OperationCatalog for ProductionCatalog {
    fn plan_op(
        &self,
        operation: OperationId,
        args: &OperationArgs,
        repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        if !self.armed.contains(&operation) {
            return Err(PlanError::NotImplemented);
        }
        match operation {
            OperationId::Commit => commit::plan_op(self.commit_env.as_ref(), args, repo, facts),
            OperationId::Snapshot => snapshot::plan_op(facts),
            #[cfg(debug_assertions)]
            _ if self.test_arms.is_some() => test_arms::plan_op(operation, args, repo, facts),
            _ => Err(PlanError::NotImplemented),
        }
    }

    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        if plan.operation == OperationId::Commit && self.armed.contains(&plan.operation) {
            return commit::step(self.commit_env.as_ref(), plan);
        }
        #[cfg(debug_assertions)]
        if let Some(arms) = &self.test_arms
            && self.armed.contains(&plan.operation)
        {
            return test_arms::step(arms, plan);
        }
        let _ = plan;
        // `snapshot` never runs as a protected step, and no other arm exists yet.
        Err(StepError::new("operation without a protected step"))
    }

    fn armed(&self) -> &[OperationId] {
        &self.armed
    }

    fn with_commit_env(&self, env: CommitEnv) -> Option<Arc<dyn OperationCatalog>> {
        Some(Arc::new(Self {
            commit_env: Some(env),
            ..self.clone()
        }))
    }
}

impl OperationsWiring {
    /// The daemon's wiring, from the process environment (read once, here).
    pub fn production() -> Self {
        Self::production_from(&|name| std::env::var(name).ok())
    }

    /// The daemon's wiring, reading the environment through `env`: the real Guardrails engine
    /// (attached to the channel when it is served), and the debug test arms only in a build
    /// with `debug_assertions` and with both `GITRAPTOR_TEST_EXECUTOR_ARMS=1` and
    /// `GITRAPTOR_PROFILE_DIR` (D14).
    pub fn production_from(env: &dyn Fn(&str) -> Option<String>) -> Self {
        let engine = crate::guardrails::plan_gate::GuardrailsEngine::default();
        let gate: Arc<dyn GuardrailsGate> = Arc::new(engine);
        let requested = Self::test_arms_requested(cfg!(debug_assertions), env);
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut arms: Vec<OperationId> = ARMS.to_vec();
        #[cfg(debug_assertions)]
        let test = requested.then(|| {
            arms.extend(test_arms::TEST_ARMS.iter().copied());
            test_arms::TestArms::from_env(env)
        });
        #[cfg(not(debug_assertions))]
        let _ = requested;
        // A governed arm is only served with a gate that decides (fail closed).
        let armed = if gate.decides() {
            arms
        } else {
            armed_without_guardrails(&arms)
        };
        Self {
            catalog: Arc::new(ProductionCatalog {
                armed,
                commit_env: None,
                #[cfg(debug_assertions)]
                test_arms: test,
            }),
            gate,
            test_layer_override: None,
            prior_deadline: DEFAULT_PRIOR_DEADLINE,
            prior_layer: None,
        }
    }

    /// Whether the debug test arms are requested: only in a build with `debug_assertions`
    /// (the caller passes `cfg!(debug_assertions)`), and only with both variables set (D14).
    pub fn test_arms_requested(
        debug_assertions: bool,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> bool {
        debug_assertions
            && env(crate::daemon::TEST_EXECUTOR_ARMS_ENV).as_deref() == Some("1")
            && env("GITRAPTOR_PROFILE_DIR").is_some_and(|p| !p.is_empty())
    }

    /// The system Git and `HOME` the `commit` arm launches with. A catalog that is not the
    /// production one is left as it is.
    #[must_use]
    pub fn with_commit_env(mut self, env: CommitEnv) -> Self {
        if let Some(catalog) = self.catalog.with_commit_env(env) {
            self.catalog = catalog;
        }
        self
    }

    /// The operations the catalog serves.
    pub fn catalog_armed(&self) -> &[OperationId] {
        self.catalog.armed()
    }
}

#[cfg(test)]
#[path = "arms_tests.rs"]
mod arms_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_wiring_has_no_governed_arm_without_guardrails() {
        // Every governed arm of `ARMS` leaves the filter when the gate never decides.
        let without = armed_without_guardrails(ARMS);
        for op in ARMS {
            if entry(*op).governed.is_some() {
                assert!(!without.contains(op), "{op:?} is served without Guardrails");
            }
        }
        // A governed operation is dropped from the arms, never served without a gate.
        let armed = armed_without_guardrails(&[OperationId::Snapshot, OperationId::MergeIntoBase]);
        assert_eq!(armed, vec![OperationId::Snapshot]);
    }
}
