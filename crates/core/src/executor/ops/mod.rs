//! The production catalog of user operations: one arm and one file per operation, in the order
//! of [`OperationId`]. Only the arms that exist are served; every other operation answers "not
//! implemented" with its story.

mod snapshot;

use std::sync::Arc;

use gitraptor_api::catalog::{OperationArgs, OperationId, entry};

use super::{NoGuardrails, OpPlan, PlanError, RepoFacts, StepPlan};
use crate::timemachine::protected::{
    DEFAULT_PRIOR_DEADLINE, OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle,
    StepError,
};

/// The operations this build serves.
const ARMS: &[OperationId] = &[OperationId::Snapshot];

/// Without state: dispatches by operation.
pub struct ProductionCatalog {
    armed: Vec<OperationId>,
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
        _args: &OperationArgs,
        _repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        if !self.armed.contains(&operation) {
            return Err(PlanError::NotImplemented);
        }
        match operation {
            OperationId::Snapshot => snapshot::plan_op(facts),
            _ => Err(PlanError::NotImplemented),
        }
    }

    fn step(&self, _plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        // `snapshot` never runs as a protected step, and no other arm exists yet.
        Err(StepError::new("operation without a protected step"))
    }
}

impl OperationsWiring {
    /// The daemon's wiring: the production catalog with the gate that never allows. An arm of a
    /// governed operation is not served while that gate is the one in place.
    pub fn production() -> Self {
        Self {
            catalog: Arc::new(ProductionCatalog {
                armed: armed_without_guardrails(ARMS),
            }),
            gate: Arc::new(NoGuardrails),
            test_layer_override: None,
            prior_deadline: DEFAULT_PRIOR_DEADLINE,
            prior_layer: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_wiring_has_no_governed_arm_without_guardrails() {
        for op in ARMS {
            assert!(entry(*op).governed.is_none(), "{op:?} is governed");
        }
        // A governed operation is dropped from the arms, never served without a gate.
        let armed = armed_without_guardrails(&[OperationId::Snapshot, OperationId::MergeIntoBase]);
        assert_eq!(armed, vec![OperationId::Snapshot]);
    }
}
