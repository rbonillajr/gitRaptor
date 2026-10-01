//! Guardrails engine: evaluates repo policies (`policy.yaml`).

/// Outcome of evaluating an operation against the repo policies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_carries_reason() {
        let d = Decision::Deny {
            reason: "force-push".into(),
        };
        assert_ne!(d, Decision::Allow);
    }
}
