//! Diff size and commit format (US-GRD-009, DS-US-GRD-009): the rules of `policies.diffSize` and
//! `policies.commitFormat`, how the sources combine and how the commits a movement brings are
//! judged against them. Pure: the I/O side reads the commits and hands over facts.
//!
//! They only add `deny` or `ask` (BR-CONS-001). The size limit: every source brings its own
//! rule and the commit must meet all of them, so the lowest limit holds and an exclusion only
//! frees the rule that declares it (D2). The format: one rule rules, the one of the first level
//! that declares it (floor, worktree, personal), so a personal level never changes the team's
//! convention (S-GRD-2). The actor enters the condition like the other policies (D3).

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{Cause, Effect, Level, Param, ParamKind, Reason, Rule};

use super::Evaluation;
use super::commit_format::{Header, Verdict};
use super::glob::{Budget, Kind, Pattern};
use super::policies::{Scope, Source};
use crate::settings::model::{CommitFormat, DiffSize, PolicyEffect};

/// The types of `@commitlint/config-conventional`, when a rule lists none.
pub const DEFAULT_TYPES: [&str; 11] = [
    "build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor", "revert", "style", "test",
];

/// The size rule one source declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRule {
    pub level: Level,
    pub scope: Scope,
    pub effect: Effect,
    pub max_lines: u32,
    /// Paths that do not count, for this rule only.
    pub exclude: Vec<Pattern>,
}

/// The format rule of the level that rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatRule {
    pub level: Level,
    pub scope: Scope,
    pub effect: Effect,
    /// The types accepted; empty when every type the rule listed was invalid (nothing passes).
    pub types: Vec<String>,
}

/// The effective rules of the two keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitPolicies {
    pub diff: Vec<DiffRule>,
    /// Usually one; two when the floor and the confirmed floor declare it, so a relaxation of
    /// the floor waits for its confirmation.
    pub format: Vec<FormatRule>,
}

/// Lines one commit changes in one file (its new path; the old one in a deletion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLines {
    pub path: String,
    pub lines: u32,
}

/// What the I/O side proved about one new commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFacts {
    /// The object id in hex.
    pub oid: String,
    pub header: Header,
    pub parents: usize,
    /// Lines per file; `None` when none was asked for.
    pub files: Option<Vec<FileLines>>,
    /// A bound cut the count: the lines are at least what `files` adds up to.
    pub partial: bool,
}

/// The commits a movement brings, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewCommits {
    pub commits: Vec<CommitFacts>,
    /// The commits could not all be read within the bounds: nothing can be said.
    pub unverifiable: bool,
}

fn effect_of(effect: Option<PolicyEffect>) -> Effect {
    match effect {
        Some(PolicyEffect::Ask) => Effect::Ask,
        Some(PolicyEffect::Deny) | None => Effect::Deny,
    }
}

fn diff_rule(level: Level, declared: &DiffSize) -> DiffRule {
    DiffRule {
        level,
        scope: Scope::declared(declared.applies_to),
        effect: effect_of(declared.effect),
        max_lines: declared.max_lines,
        // A pattern the parser already dropped never arrives; one that still fails to build is
        // skipped, so a bad entry never frees more than it names.
        exclude: declared
            .exclude
            .iter()
            .flatten()
            .filter_map(|raw| Pattern::new(raw, Kind::Path).ok())
            .collect(),
    }
}

fn format_rule(level: Level, declared: &CommitFormat) -> FormatRule {
    FormatRule {
        level,
        scope: Scope::declared(declared.applies_to),
        effect: effect_of(declared.effect),
        types: declared
            .types
            .clone()
            .unwrap_or_else(|| DEFAULT_TYPES.iter().map(|t| (*t).to_owned()).collect()),
    }
}

/// Adds `rule` unless a rule of a level above already says the same: the worktree of a repo
/// reads the team's file too, and one broken limit must not be named twice.
fn push_rule(rules: &mut Vec<DiffRule>, rule: DiffRule) {
    let same = |r: &DiffRule| {
        r.max_lines == rule.max_lines
            && r.scope == rule.scope
            && r.effect == rule.effect
            && r.exclude == rule.exclude
    };
    if !rules.iter().any(same) {
        rules.push(rule);
    }
}

/// The rules of the sources, in the order floor, confirmed floor, worktree, profile, local
/// (D2). The personal level is the local one when it declares the key, else the profile one.
pub fn combine(sources: &[Source<'_>]) -> CommitPolicies {
    let mut out = CommitPolicies::default();
    let (mut profile_diff, mut local_diff) = (None, None);
    let (mut floor_format, mut worktree_format) = (Vec::new(), None);
    let (mut profile_format, mut local_format) = (None, None);
    for source in sources {
        let Some(policies) = source.settings.and_then(|s| s.policies.as_ref()) else {
            continue;
        };
        if let Some(declared) = &policies.diff_size {
            let rule = diff_rule(source.level, declared);
            match source.level {
                Level::Profile => profile_diff = Some(rule),
                Level::Local => local_diff = Some(rule),
                _ => push_rule(&mut out.diff, rule),
            }
        }
        if let Some(declared) = &policies.commit_format {
            let rule = format_rule(source.level, declared);
            match source.level {
                Level::Worktree => worktree_format = Some(rule),
                Level::Profile => profile_format = Some(rule),
                Level::Local => local_format = Some(rule),
                _ => floor_format.push(rule),
            }
        }
    }
    if let Some(rule) = local_diff.or(profile_diff) {
        push_rule(&mut out.diff, rule);
    }
    out.format = if !floor_format.is_empty() {
        floor_format
    } else {
        worktree_format
            .or(local_format)
            .or(profile_format)
            .into_iter()
            .collect()
    };
    out
}

impl CommitPolicies {
    pub fn is_empty(&self) -> bool {
        self.diff.is_empty() && self.format.is_empty()
    }

    /// Whether the commits of a movement by `actor` are worth reading: only when a rule of
    /// either key governs it (D15).
    pub fn needs_commit_facts(&self, actor: Option<AgentKind>) -> bool {
        self.needs_lines(actor) || self.format.iter().any(|r| r.scope.governs(actor))
    }

    /// Whether the lines of the commits are worth counting: only when a size rule governs the
    /// actor (with only the format no tree is read).
    pub fn needs_lines(&self, actor: Option<AgentKind>) -> bool {
        self.diff.iter().any(|r| r.scope.governs(actor))
    }

    /// The rules that do not need an actor (`everyone`): what a client that cannot tell the
    /// actor applies in degraded mode (D14).
    pub fn everyone_only(&self) -> Self {
        Self {
            diff: self
                .diff
                .iter()
                .filter(|r| r.scope == Scope::Everyone)
                .cloned()
                .collect(),
            format: self
                .format
                .iter()
                .filter(|r| r.scope == Scope::Everyone)
                .cloned()
                .collect(),
        }
    }
}

fn short(oid: &str) -> String {
    oid.chars().take(12).collect()
}

/// The commit a reason names, if it exists yet: a commit a plan is about to make has no oid,
/// and its reason names none.
fn oid_params(oid: &str) -> Vec<Param> {
    if oid.is_empty() {
        Vec::new()
    } else {
        vec![Param::new(ParamKind::Oid, short(oid))]
    }
}

fn reason(rule: Rule, level: Level, cause: Option<Cause>, params: Vec<Param>) -> Reason {
    Reason {
        rule,
        level,
        cause,
        params,
    }
}

/// The commits of one movement by `actor`: one reason for each broken rule, naming the first
/// commit that breaks it (D11). What cannot be counted or read is denied, never allowed.
pub fn apply(
    out: &mut Evaluation,
    commits: &NewCommits,
    actor: Option<AgentKind>,
    rules: &CommitPolicies,
    budget: &mut Budget,
) {
    for rule in rules.diff.iter().filter(|r| r.scope.governs(actor)) {
        diff_size(out, commits, rule, budget);
    }
    for rule in rules.format.iter().filter(|r| r.scope.governs(actor)) {
        commit_format(out, commits, rule);
    }
}

/// The lines of `commit` that count for `rule`; `Err` when they cannot be known.
fn counted(commit: &CommitFacts, rule: &DiffRule, budget: &mut Budget) -> Result<u64, ()> {
    let files = commit.files.as_deref().ok_or(())?;
    let mut total = 0_u64;
    for file in files {
        let mut excluded = false;
        for pattern in &rule.exclude {
            if pattern.matches_path(&file.path, budget).map_err(|_| ())? {
                excluded = true;
                break;
            }
        }
        if !excluded {
            total = total.saturating_add(u64::from(file.lines));
        }
    }
    Ok(total)
}

fn diff_size(out: &mut Evaluation, commits: &NewCommits, rule: &DiffRule, budget: &mut Budget) {
    let limit = Param::new(ParamKind::Limit, rule.max_lines.to_string());
    let unverifiable = |params: Vec<Param>| {
        reason(
            Rule::DiffSize,
            rule.level,
            Some(Cause::Unverifiable),
            params,
        )
    };
    if commits.unverifiable {
        out.add(Effect::Deny, unverifiable(vec![limit]));
        return;
    }
    let mut first: Option<(&CommitFacts, u64)> = None;
    let mut unproven: Option<&CommitFacts> = None;
    let mut over = 0_usize;
    // A merge brings commits already judged: its size does not count (D6).
    for commit in commits.commits.iter().filter(|c| c.parents < 2) {
        match counted(commit, rule, budget) {
            Ok(n) if n > u64::from(rule.max_lines) => {
                over += 1;
                first.get_or_insert((commit, n));
            }
            Ok(_) if !commit.partial => {}
            // Cut below the limit, or not counted at all: nothing proves it fits.
            Ok(_) | Err(()) => {
                unproven.get_or_insert(commit);
            }
        }
    }
    if let Some((commit, lines)) = first {
        let at_least = if commit.partial { "≥" } else { "" };
        let mut params = oid_params(&commit.oid);
        params.push(Param::new(ParamKind::Lines, format!("{at_least}{lines}")));
        params.push(limit.clone());
        if over > 1 {
            params.push(Param::new(ParamKind::Commits, over.to_string()));
        }
        out.add(
            rule.effect,
            reason(Rule::DiffSize, rule.level, None, params),
        );
    }
    if let Some(commit) = unproven
        && (first.is_none() || rule.effect != Effect::Deny)
    {
        let mut params = oid_params(&commit.oid);
        params.push(limit);
        out.add(Effect::Deny, unverifiable(params));
    }
}

fn commit_format(out: &mut Evaluation, commits: &NewCommits, rule: &FormatRule) {
    if commits.unverifiable {
        out.add(
            Effect::Deny,
            reason(
                Rule::CommitFormat,
                rule.level,
                Some(Cause::Unverifiable),
                Vec::new(),
            ),
        );
        return;
    }
    let types = || Param::new(ParamKind::Types, rule.types.join(", "));
    for commit in &commits.commits {
        let oid = || oid_params(&commit.oid);
        let kind_allowed = commit
            .header
            .kind
            .as_deref()
            .is_none_or(|k| rule.types.iter().any(|t| t == k));
        let (cause, params) = match commit.header.verdict {
            Verdict::NotConventional => (None, [oid(), vec![types()]].concat()),
            _ if !kind_allowed => (Some(Cause::TypeNotAllowed), [oid(), vec![types()]].concat()),
            Verdict::BodyNotSeparated => (Some(Cause::BodyNotSeparated), oid()),
            Verdict::Conforms => continue,
        };
        out.add(
            rule.effect,
            reason(Rule::CommitFormat, rule.level, cause, params),
        );
        return;
    }
}
