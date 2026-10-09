//! Per-case outcomes, the KPI and the gate.

use std::fmt::Write as _;

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

/// Escapes text that came from a case file or a response, and cuts it.
fn clean(text: &str, max: usize) -> String {
    let escaped: String = text.chars().flat_map(char::escape_debug).collect();
    let mut out: String = escaped.chars().take(max).collect();
    if escaped.chars().count() > max {
        out.push('…');
    }
    out.replace('|', "\\|")
}

impl Report {
    fn count(&self, keep: impl Fn(&Row) -> bool) -> usize {
        self.rows.iter().filter(|row| keep(row)).count()
    }

    /// Rows that ran and count for the KPI: not pending and not a known gap.
    pub fn executed(&self) -> usize {
        self.count(|row| matches!(row.outcome, Outcome::Rejected | Outcome::Failed(_)))
    }

    pub fn rejected(&self) -> usize {
        self.count(|row| matches!(row.outcome, Outcome::Rejected))
    }

    pub fn pending(&self) -> usize {
        self.count(|row| matches!(row.outcome, Outcome::Pending(_)))
    }

    /// Known gaps, counted apart.
    pub fn known_gap(&self) -> usize {
        self.count(|row| matches!(row.outcome, Outcome::KnownGap(_)))
    }

    pub fn executed_in(&self, tier: Tier) -> usize {
        self.count(|row| {
            row.tier == tier && matches!(row.outcome, Outcome::Rejected | Outcome::Failed(_))
        })
    }

    /// Tenths of a percent, rounded DOWN: 1000 only when every executed case was rejected; 0
    /// with none.
    pub fn kpi_permille(&self) -> u32 {
        let executed = self.executed();
        if executed == 0 {
            return 0;
        }
        u32::try_from(self.rejected() * 1000 / executed).unwrap_or(1000)
    }

    pub fn summary_line(&self) -> String {
        let permille = self.kpi_permille();
        let mut line = format!(
            "MCP security corpus (Q-MCP-18): {}/{} rejected ({}.{} %) on {}; {} pending",
            self.rejected(),
            self.executed(),
            permille / 10,
            permille % 10,
            clean(&self.os, 40),
            self.pending()
        );
        let gaps = self.known_gap();
        if gaps > 0 {
            let _ = write!(line, "; {gaps} known gap");
        }
        line
    }

    pub fn markdown(&self) -> String {
        let mut out = format!(
            "<!-- mcp-corpus os={} executed={} rejected={} pending={} -->\n\n{}\n\n",
            clean(&self.os, 40),
            self.executed(),
            self.rejected(),
            self.pending(),
            self.summary_line()
        );
        out.push_str("| case | tier | outcome |\n|---|---|---|\n");
        for row in &self.rows {
            let tier = match row.tier {
                Tier::Server => "server",
                Tier::Engine => "engine",
            };
            let outcome = match &row.outcome {
                Outcome::Rejected => "rejected".to_owned(),
                Outcome::Failed(_) => "FAILED".to_owned(),
                Outcome::Pending(why) => format!("pending ({})", clean(why, 60)),
                Outcome::KnownGap(gap) => format!("known gap ({})", clean(gap, 60)),
            };
            let _ = writeln!(out, "| {} | {tier} | {outcome} |", clean(&row.id, 80));
        }
        let failures = self.failure_lines();
        if !failures.is_empty() {
            out.push_str("\nFailures:\n\n");
            for line in failures {
                let _ = writeln!(out, "- {line}");
            }
        }
        out
    }

    fn failure_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for row in &self.rows {
            if let Outcome::Failed(failures) = &row.outcome {
                if failures.is_empty() {
                    lines.push(format!("{}: failed", clean(&row.id, 80)));
                }
                for failure in failures {
                    lines.push(format!("{}: {failure}", clean(&row.id, 80)));
                }
            }
        }
        lines
    }

    /// # Errors
    /// When nothing was executed or one executed case was not rejected.
    pub fn gate(&self) -> Result<(), String> {
        if self.executed() == 0 {
            return Err("no corpus case was executed".to_owned());
        }
        let failures = self.failure_lines();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(format!("{}\n{}", self.summary_line(), failures.join("\n")))
        }
    }
}
