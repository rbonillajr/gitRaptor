//! Per-case outcomes, the KPI and the gate.

use super::case::Tier;
use super::judge::Failure;

/// What happened to one case.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Rejected,
    Failed(Vec<Failure>),
    Pending(String),
    /// Expected to get through today (a known server gap, with its reference). Counted apart:
    /// neither a rejection nor part of the KPI's denominator.
    KnownGap(String),
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub tier: Tier,
    pub outcome: Outcome,
}

/// The corpus run on one OS.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub os: String,
    pub rows: Vec<Row>,
}

impl Report {
    /// Rows that ran and count for the KPI: not pending and not a known gap.
    pub fn executed(&self) -> usize {
        0
    }

    pub fn rejected(&self) -> usize {
        0
    }

    pub fn pending(&self) -> usize {
        0
    }

    /// Known gaps, counted apart.
    pub fn known_gap(&self) -> usize {
        0
    }

    pub fn executed_in(&self, tier: Tier) -> usize {
        let _ = tier;
        0
    }

    /// Tenths of a percent, rounded DOWN: 1000 only when every executed case was rejected; 0
    /// with none.
    pub fn kpi_permille(&self) -> u32 {
        0
    }

    pub fn summary_line(&self) -> String {
        String::new()
    }

    pub fn markdown(&self) -> String {
        String::new()
    }

    /// # Errors
    /// When nothing was executed or one executed case was not rejected.
    pub fn gate(&self) -> Result<(), String> {
        Ok(())
    }
}
