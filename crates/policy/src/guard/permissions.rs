//! Permissions per operation (BR-VAL-002): which operations of the catalog a ref update is
//! ([`classify`]), and the reasons a permission adds to an evaluation ([`apply`]). A permission
//! only adds `ask` or `deny` on top of what the rest decides; what cannot be proved and would be
//! needed for a permission other than `allow` is denied as `unverifiable`, never allowed. Pure:
//! the daemon reads the facts (the chain of `git` processes, the fresh commits).

use gitraptor_api::guard::{Cause, Effect, Level, Param, Reason, Rule};

use super::Evaluation;
use super::git_command::GitCommand;
use crate::settings::SourceKind;
use crate::settings::model::Operation;
use crate::team::{EffectivePermissions, Permission, RuleSource};

/// The kind of a `refs/heads/*` line of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// The branch is deleted (not a prune of `pack-refs`).
    Delete,
    /// The branch is created.
    Create,
    /// The branch moves.
    Update,
}

/// The consecutive `git` processes above the hook, nearest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chain {
    commands: Vec<GitCommand>,
}

impl Chain {
    pub fn new(commands: Vec<GitCommand>) -> Self {
        Self { commands }
    }

    /// The commands, nearest first.
    pub fn commands(&self) -> &[GitCommand] {
        &self.commands
    }

    /// Every process of the chain is read with certainty and one of them is a `clone`: the
    /// refs it writes come from the remote and were judged there (US-GRD-009, D6). A `fetch`
    /// is not exempt: it can write `refs/heads/*` from any local ref.
    pub fn certainly_clones(&self) -> bool {
        !self.uncertain() && self.commands.iter().any(|c| c.is("clone", None))
    }

    /// Every process is read with certainty and the movement is a `fetch`, `pull` or `merge`:
    /// the commits a remote-tracking branch already holds are not new to the repo.
    pub fn certainly_takes_remote_commits(&self) -> bool {
        !self.uncertain() && matches!(self.best(), Kind::Fetch | Kind::Merge)
    }

    /// The highest-precedence kind read with certainty.
    fn best(&self) -> Kind {
        self.commands
            .iter()
            .map(Kind::of)
            .max()
            .unwrap_or(Kind::Other)
    }

    /// Some process of the chain could not be read with certainty (an empty chain proves
    /// nothing either).
    fn uncertain(&self) -> bool {
        self.commands.is_empty() || self.commands.contains(&GitCommand::Unknown)
    }
}

/// What the command that moves a branch is, in precedence order (§ 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    /// Anything else, an unknown command included.
    Other,
    /// `fetch` or `clone`: the commits come from the remote.
    Fetch,
    /// `merge` or `pull`.
    Merge,
    Rebase,
    WorktreeAdd,
}

impl Kind {
    fn of(command: &GitCommand) -> Self {
        match command.subcommand() {
            Some("worktree") if command.is("worktree", Some("add")) => Self::WorktreeAdd,
            Some("rebase") => Self::Rebase,
            Some("merge" | "pull") => Self::Merge,
            Some("fetch" | "clone") => Self::Fetch,
            _ => Self::Other,
        }
    }
}

/// The commits a line brings that no other branch holds (DS-US-GRD-008 D5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fresh {
    pub commits: usize,
    /// Fresh commits with two or more parents.
    pub merges: usize,
    /// They could not be counted within the bounds.
    pub unverifiable: bool,
}

/// The operations of the catalog one line is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Classified {
    /// Certain operations, sorted, without repeats.
    pub operations: Vec<Operation>,
    /// A fact that a permission other than `allow` needed could not be proved.
    pub unverifiable: bool,
    /// The operations the line may be and that could not be ruled out (the ones that make it
    /// `unverifiable`), sorted, without repeats.
    pub unproved: Vec<Operation>,
}

/// Classifies one `refs/heads/*` line (§ 3.1). `fresh` is `None` when the commits were not read;
/// `needs` says which permissions are not `allow`, so an unprovable fact only matters for them.
pub fn classify(
    line: LineKind,
    chain: &Chain,
    fresh: Option<&Fresh>,
    needs: &EffectivePermissions,
) -> Classified {
    let mut out = Classified::default();
    let strict = |op: Operation| needs.permission(op) != Permission::Allow;
    if line == LineKind::Delete {
        out.operations.push(Operation::BranchDelete);
        return out;
    }
    let uncertain = chain.uncertain();
    let mut kind = chain.best();
    if line == LineKind::Create {
        if kind == Kind::WorktreeAdd {
            out.operations.push(Operation::WorktreeAdd);
        } else if uncertain && strict(Operation::WorktreeAdd) {
            out.unproved.push(Operation::WorktreeAdd);
        }
    }
    // The commits may be fresh: counted as some, or not counted at all.
    let maybe_fresh = fresh.is_none_or(|f| f.unverifiable || f.commits > 0);
    if uncertain && maybe_fresh {
        // An unknown process may be a rebase or a merge that outranks what was read.
        if kind < Kind::Rebase && strict(Operation::Rebase) {
            out.unproved.push(Operation::Rebase);
        }
        if kind < Kind::Merge && strict(Operation::Merge) {
            out.unproved.push(Operation::Merge);
        }
    }
    // Under `worktree add` (it outranks the rest) its fresh commits are read as any other
    // command's.
    if kind == Kind::WorktreeAdd {
        kind = Kind::Other;
    }
    match kind {
        Kind::Rebase => out.operations.push(Operation::Rebase),
        Kind::Fetch | Kind::WorktreeAdd => {}
        Kind::Merge | Kind::Other => match fresh {
            None => {}
            Some(f) if f.unverifiable => {
                for op in [Operation::Commit, Operation::Merge] {
                    if strict(op) {
                        out.unproved.push(op);
                    }
                }
            }
            Some(f) if kind == Kind::Merge => {
                if f.commits > 0 {
                    out.operations.push(Operation::Merge);
                    // The merge command creates at most one merge commit of its own.
                    if f.commits > f.merges.min(1) {
                        out.operations.push(Operation::Commit);
                    }
                }
            }
            Some(f) => {
                if f.commits > 0 {
                    out.operations.push(Operation::Commit);
                }
                if f.merges > 0 {
                    out.operations.push(Operation::Merge);
                }
            }
        },
    }
    out.operations.sort();
    out.operations.dedup();
    out.unproved.sort();
    out.unproved.dedup();
    out.unverifiable = !out.unproved.is_empty();
    out
}

/// The rule of an operation of the catalog.
pub fn rule(op: Operation) -> Rule {
    match op {
        Operation::Commit => Rule::PermissionCommit,
        Operation::Push => Rule::PermissionPush,
        Operation::ForcePush => Rule::PermissionForcePush,
        Operation::ResetHard => Rule::PermissionResetHard,
        Operation::BranchDelete => Rule::PermissionBranchDelete,
        Operation::Rebase => Rule::PermissionRebase,
        Operation::Merge => Rule::PermissionMerge,
        Operation::WorktreeAdd => Rule::PermissionWorktreeAdd,
        Operation::WorktreeRemove => Rule::PermissionWorktreeRemove,
    }
}

/// The level a source of a rule is named with; the safe minimum gives none (it has its own rule).
fn level(source: RuleSource) -> Option<Level> {
    match source {
        RuleSource::SafeMinimum => None,
        RuleSource::Source(kind) => Some(match kind {
            SourceKind::Floor | SourceKind::ConfirmedFloor | SourceKind::Team => Level::Floor,
            SourceKind::Worktree => Level::Worktree,
            SourceKind::Profile => Level::Profile,
            SourceKind::Local => Level::Local,
        }),
    }
}

/// Adds the permission of `op`: one reason per level that produces the maximum, `ask` or `deny`
/// as the permission says; nothing for `allow` or when only the safe minimum produces it.
/// `unverifiable` denies with that cause whatever the permission other than `allow` is.
pub fn apply(
    out: &mut Evaluation,
    op: Operation,
    unverifiable: bool,
    permissions: &EffectivePermissions,
    params: &[Param],
) {
    let Some(rule_of) = permissions.operations.get(&op) else {
        return;
    };
    let effect = match rule_of.permission {
        Permission::Allow => return,
        Permission::Ask if !unverifiable => Effect::Ask,
        Permission::Ask | Permission::Deny => Effect::Deny,
    };
    for source in &rule_of.sources {
        if let Some(level) = level(*source) {
            out.add(
                effect,
                Reason {
                    rule: rule(op),
                    level,
                    cause: unverifiable.then_some(Cause::Unverifiable),
                    params: params.to_vec(),
                },
            );
        }
    }
}

/// Applies what one line was classified as.
pub fn apply_classified(
    out: &mut Evaluation,
    classified: &Classified,
    permissions: &EffectivePermissions,
    params: &[Param],
) {
    for op in &classified.operations {
        apply(out, *op, false, permissions, params);
    }
    for op in &classified.unproved {
        apply(out, *op, true, permissions, params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::Settings;
    use crate::team::combine;

    fn needs(json: &str) -> EffectivePermissions {
        let s: Settings = serde_json::from_str(json).expect("settings");
        combine(Some((SourceKind::Floor, &s)), true, &[])
    }

    fn certain(sub: &'static str) -> GitCommand {
        GitCommand::Certain(sub, None)
    }

    #[test]
    fn an_unknown_process_above_a_certain_fetch_can_still_hide_a_rebase() {
        let chain = Chain::new(vec![GitCommand::Unknown, certain("fetch")]);
        let fresh = Fresh {
            commits: 1,
            ..Fresh::default()
        };
        let c = classify(
            LineKind::Update,
            &chain,
            Some(&fresh),
            &needs(r#"{"permissions":{"deny":["rebase"]}}"#),
        );
        assert_eq!(c.unproved, [Operation::Rebase]);
        assert!(c.operations.is_empty());
    }

    #[test]
    fn a_merge_ff_only_under_pull_that_brings_nothing_is_nothing() {
        let chain = Chain::new(vec![certain("merge"), certain("pull")]);
        let c = classify(
            LineKind::Update,
            &chain,
            Some(&Fresh::default()),
            &needs(r#"{"permissions":{"deny":["merge","commit","rebase"]}}"#),
        );
        assert_eq!(c, Classified::default());
    }

    #[test]
    fn an_unproved_ask_is_denied_as_unverifiable() {
        let p = needs(r#"{"permissions":{"ask":["merge"]}}"#);
        let mut out = Evaluation::allow();
        apply(&mut out, Operation::Merge, true, &p, &[]);
        assert_eq!(out.effect, Effect::Deny);
        assert_eq!(out.reasons[0].cause, Some(Cause::Unverifiable));
    }
}
