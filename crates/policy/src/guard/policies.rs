//! Protected branches and forbidden paths (US-GRD-008, DS-US-GRD-008): the rules of
//! `policies.protectedBranches` and `policies.forbiddenPaths` and how the sources combine.
//!
//! They only harden (BR-CONS-001): the effective rules are the union of what every readable
//! source declares, each keeping its level and its `appliesTo`. The actor enters the condition
//! (D3): an `agents` rule denies only when the operation is an agent's; an `everyone` rule
//! denies whoever it is. With "unattributed" an `agents` rule lets the operation through
//! (Q-GRD-35).

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{Cause, Effect, Level, Param, ParamKind, Reason, Rule};

use super::Evaluation;
use super::glob::{Budget, Exceeded, Kind, Pattern};
use super::refs;
use crate::settings::model::{AppliesTo, PatternPolicy, Settings};

/// Who a rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Agents,
    Everyone,
}

impl Scope {
    fn of(policy: &PatternPolicy) -> Self {
        match policy.applies_to {
            Some(AppliesTo::Everyone) => Self::Everyone,
            Some(AppliesTo::Agents) | None => Self::Agents,
        }
    }

    /// Whether the rule governs an operation of `actor` (`None` = unattributed).
    fn governs(self, actor: Option<AgentKind>) -> bool {
        self == Self::Everyone || actor.is_some()
    }
}

/// The patterns one source declared for one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    pub level: Level,
    pub scope: Scope,
    pub patterns: Vec<Pattern>,
}

/// The effective protected branches and forbidden paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policies {
    pub branches: Vec<Rules>,
    pub paths: Vec<Rules>,
    /// The team configuration could not be read at all (an I/O error, a repo whose main branch
    /// cannot be resolved): an agent's movement of a branch cannot be judged and is denied, as
    /// anything that cannot be read never reads as "no rules" (SEC-GRD-17).
    pub unreadable: bool,
}

/// One level that may declare the keys.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub level: Level,
    pub settings: Option<&'a Settings>,
}

/// The union of the sources (D2). A pattern the parser already dropped never arrives; one that
/// still fails to build is skipped, never relaxing the rest.
pub fn combine(sources: &[Source<'_>]) -> Policies {
    let mut out = Policies::default();
    for source in sources {
        let Some(policies) = source.settings.and_then(|s| s.policies.as_ref()) else {
            continue;
        };
        for (policy, kind, into) in [
            (
                policies.protected_branches.as_ref(),
                Kind::Branch,
                &mut out.branches,
            ),
            (
                policies.forbidden_paths.as_ref(),
                Kind::Path,
                &mut out.paths,
            ),
        ] {
            let Some(policy) = policy else {
                continue;
            };
            let patterns: Vec<Pattern> = policy
                .patterns
                .iter()
                .flatten()
                .filter_map(|raw| Pattern::new(raw, kind).ok())
                .collect();
            if !patterns.is_empty() {
                into.push(Rules {
                    level: source.level,
                    scope: Scope::of(policy),
                    patterns,
                });
            }
        }
    }
    out
}

impl Policies {
    pub fn is_empty(&self) -> bool {
        self.branches.is_empty() && self.paths.is_empty() && !self.unreadable
    }

    /// The rules of a configuration that could not be read.
    pub fn unreadable() -> Self {
        Self {
            unreadable: true,
            ..Self::default()
        }
    }

    /// Whether the commits of a movement by `actor` are worth reading for the forbidden paths:
    /// only when a forbidden-path rule governs it (the protected configuration has its own
    /// read).
    pub fn needs_paths(&self, actor: Option<AgentKind>) -> bool {
        self.paths.iter().any(|r| r.scope.governs(actor))
    }

    /// The same rules keeping only the ones that do not need an actor (`everyone`): what a
    /// client that cannot tell the actor evaluates in degraded mode (D11).
    pub fn everyone_only(&self) -> Self {
        let keep = |rules: &[Rules]| -> Vec<Rules> {
            rules
                .iter()
                .filter(|r| r.scope == Scope::Everyone)
                .cloned()
                .collect()
        };
        Self {
            branches: keep(&self.branches),
            paths: keep(&self.paths),
            // Without an actor the person and the agent are told apart by nothing: a client
            // that cannot read the configuration does not guess.
            unreadable: false,
        }
    }

    /// The same rules as if each governed whoever moves the ref (`everyone`): what a client that
    /// cannot tell the actor applies to the refs Guardrails does not govern, so a rule for agents
    /// is not skipped because the agent cannot be told apart. `unreadable` is dropped.
    pub fn every_rule(&self) -> Self {
        let widen = |rules: &[Rules]| -> Vec<Rules> {
            rules
                .iter()
                .cloned()
                .map(|mut r| {
                    r.scope = Scope::Everyone;
                    r
                })
                .collect()
        };
        Self {
            branches: widen(&self.branches),
            paths: widen(&self.paths),
            unreadable: false,
        }
    }
}

/// What the I/O side proved about the commits a movement brings (DS-US-GRD-008 D5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Touched {
    /// Paths a new commit of the movement modifies, creates or deletes.
    pub paths: Vec<String>,
    /// The commits could not all be read or counted within the bounds: nothing can be said.
    pub unverifiable: bool,
}

/// `refname` (a full ref) moved, by `actor`: one reason for each protected pattern it matches.
pub fn protected_branch(
    out: &mut Evaluation,
    refname: &str,
    actor: Option<AgentKind>,
    policies: &Policies,
    budget: &mut Budget,
) {
    let Some(branch) = refname.strip_prefix("refs/heads/") else {
        return;
    };
    if policies.unreadable && actor.is_some() {
        out.add(
            Effect::Deny,
            Reason {
                rule: Rule::ProtectedBranch,
                level: Level::System,
                cause: Some(Cause::Unverifiable),
                params: vec![Param::new(ParamKind::Branch, branch)],
            },
        );
    }
    for rules in policies.branches.iter().filter(|r| r.scope.governs(actor)) {
        for pattern in &rules.patterns {
            match pattern.matches_branch(branch, budget) {
                Ok(true) => out.add(
                    Effect::Deny,
                    Reason {
                        rule: Rule::ProtectedBranch,
                        level: rules.level,
                        cause: None,
                        params: vec![
                            Param::new(ParamKind::Branch, refs::short(refname)),
                            Param::new(ParamKind::Pattern, pattern.raw()),
                        ],
                    },
                ),
                Ok(false) => {}
                // Never "no match" for a name that cannot be compared.
                Err(Exceeded) => out.add(
                    Effect::Deny,
                    Reason {
                        rule: Rule::ProtectedBranch,
                        level: rules.level,
                        cause: Some(Cause::Unverifiable),
                        params: vec![Param::new(ParamKind::Branch, refs::short(refname))],
                    },
                ),
            }
        }
    }
}

/// The commits of one movement by `actor`: one reason for each forbidden pattern some changed
/// path matches (with the first such path), or `unverifiable` when they cannot be read.
pub fn forbidden_paths(
    out: &mut Evaluation,
    touched: &Touched,
    actor: Option<AgentKind>,
    policies: &Policies,
    budget: &mut Budget,
) {
    let mut applicable = policies.paths.iter().filter(|r| r.scope.governs(actor));
    if touched.unverifiable {
        if let Some(rules) = applicable.next() {
            out.add(
                Effect::Deny,
                Reason {
                    rule: Rule::ForbiddenPath,
                    level: rules.level,
                    cause: Some(Cause::Unverifiable),
                    params: Vec::new(),
                },
            );
        }
        return;
    }
    for rules in applicable {
        for pattern in &rules.patterns {
            let hit = first_match(pattern, &touched.paths, budget);
            match hit {
                Ok(Some(path)) => out.add(
                    Effect::Deny,
                    Reason {
                        rule: Rule::ForbiddenPath,
                        level: rules.level,
                        cause: None,
                        params: vec![
                            Param::new(ParamKind::Path, path),
                            Param::new(ParamKind::Pattern, pattern.raw()),
                        ],
                    },
                ),
                Ok(None) => {}
                // Running out of work or a path too long to compare: never "no match".
                Err(Exceeded) => out.add(
                    Effect::Deny,
                    Reason {
                        rule: Rule::ForbiddenPath,
                        level: rules.level,
                        cause: Some(Cause::Unverifiable),
                        params: Vec::new(),
                    },
                ),
            }
        }
    }
}

fn first_match<'a>(
    pattern: &Pattern,
    paths: &'a [String],
    budget: &mut Budget,
) -> Result<Option<&'a str>, Exceeded> {
    for path in paths {
        if pattern.matches_path(path, budget)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::Policies as Declared;

    fn settings(json: &str) -> Settings {
        // The strict parser is not the subject here: the types are.
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let policies = value["policies"].clone();
        let get = |key: &str| {
            policies.get(key).map(|p| PatternPolicy {
                patterns: p["patterns"]
                    .as_array()
                    .map(|a| a.iter().map(|s| s.as_str().unwrap().to_owned()).collect()),
                applies_to: match p["appliesTo"].as_str() {
                    Some("everyone") => Some(AppliesTo::Everyone),
                    Some("agents") => Some(AppliesTo::Agents),
                    _ => None,
                },
            })
        };
        Settings {
            policies: Some(Declared {
                commit_authorship: None,
                protected_branches: get("protectedBranches"),
                forbidden_paths: get("forbiddenPaths"),
            }),
            ..Settings::default()
        }
    }

    const AGENT: Option<AgentKind> = Some(AgentKind::ClaudeCode);

    fn rules(json: &str, level: Level) -> Policies {
        combine(&[Source {
            level,
            settings: Some(&settings(json)),
        }])
    }

    fn denied_branch(p: &Policies, refname: &str, actor: Option<AgentKind>) -> Vec<Reason> {
        let mut out = Evaluation::allow();
        protected_branch(&mut out, refname, actor, p, &mut Budget::default());
        out.reasons
    }

    fn denied_paths(p: &Policies, paths: &[&str], actor: Option<AgentKind>) -> Vec<Reason> {
        let touched = Touched {
            paths: paths.iter().map(|s| (*s).to_owned()).collect(),
            unverifiable: false,
        };
        let mut out = Evaluation::allow();
        forbidden_paths(&mut out, &touched, actor, p, &mut Budget::default());
        out.reasons
    }

    #[test]
    fn an_agents_rule_denies_the_agent_and_lets_the_person_through() {
        let p = rules(
            r#"{"policies":{"protectedBranches":{"patterns":["main","release/*"]}}}"#,
            Level::Floor,
        );
        let reasons = denied_branch(&p, "refs/heads/main", AGENT);
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].rule, Rule::ProtectedBranch);
        assert_eq!(reasons[0].level, Level::Floor);
        let named: Vec<(ParamKind, &str)> = reasons[0]
            .params
            .iter()
            .map(|p| (p.kind, p.value.raw()))
            .collect();
        assert_eq!(
            named,
            [(ParamKind::Branch, "main"), (ParamKind::Pattern, "main")]
        );
        assert_eq!(denied_branch(&p, "refs/heads/release/1.0", AGENT).len(), 1);
        assert!(denied_branch(&p, "refs/heads/feat-x", AGENT).is_empty());
        // Unattributed: the person.
        assert!(denied_branch(&p, "refs/heads/main", None).is_empty());
        // Tags and other refs are not branches.
        assert!(denied_branch(&p, "refs/tags/main", AGENT).is_empty());
    }

    #[test]
    fn an_everyone_rule_denies_the_person_too() {
        let p = rules(
            r#"{"policies":{"protectedBranches":{"patterns":["main"],"appliesTo":"everyone"}}}"#,
            Level::Floor,
        );
        assert_eq!(denied_branch(&p, "refs/heads/main", None).len(), 1);
        assert_eq!(denied_branch(&p, "refs/heads/main", AGENT).len(), 1);
    }

    #[test]
    fn branch_aliases_are_the_branch() {
        let p = rules(
            r#"{"policies":{"protectedBranches":{"patterns":["main"]}}}"#,
            Level::Floor,
        );
        assert_eq!(denied_branch(&p, "refs/heads/Main", AGENT).len(), 1);
        assert_eq!(denied_branch(&p, "refs/heads/MAIN", AGENT).len(), 1);
    }

    #[test]
    fn the_sources_union_and_none_removes_anothers_pattern() {
        let team = settings(r#"{"policies":{"protectedBranches":{"patterns":["main"]}}}"#);
        let profile = settings(
            r#"{"policies":{"protectedBranches":{"patterns":["stable"]},
                            "forbiddenPaths":{"patterns":["*.pem"]}}}"#,
        );
        let empty = settings(r#"{"policies":{"protectedBranches":{"patterns":[]}}}"#);
        let p = combine(&[
            Source {
                level: Level::Floor,
                settings: Some(&team),
            },
            Source {
                level: Level::Profile,
                settings: Some(&profile),
            },
            Source {
                level: Level::Worktree,
                settings: Some(&empty),
            },
        ]);
        let by = |name: &str| denied_branch(&p, name, AGENT);
        assert_eq!(by("refs/heads/main")[0].level, Level::Floor);
        assert_eq!(by("refs/heads/stable")[0].level, Level::Profile);
        assert_eq!(denied_paths(&p, &["a/key.pem"], AGENT).len(), 1);
    }

    #[test]
    fn the_same_pattern_at_two_levels_names_both_and_everyone_wins() {
        let team = settings(r#"{"policies":{"protectedBranches":{"patterns":["main"]}}}"#);
        let local = settings(
            r#"{"policies":{"protectedBranches":{"patterns":["main"],"appliesTo":"everyone"}}}"#,
        );
        let p = combine(&[
            Source {
                level: Level::Floor,
                settings: Some(&team),
            },
            Source {
                level: Level::Local,
                settings: Some(&local),
            },
        ]);
        // The person is held by the stricter rule only.
        let person = denied_branch(&p, "refs/heads/main", None);
        assert_eq!(person.len(), 1);
        assert_eq!(person[0].level, Level::Local);
        // The agent is held by both, named together (BR-CALC-001).
        let agent = denied_branch(&p, "refs/heads/main", AGENT);
        assert_eq!(agent.len(), 2);
    }

    #[test]
    fn forbidden_paths_name_the_first_path_of_each_pattern() {
        let p = rules(
            r#"{"policies":{"forbiddenPaths":{"patterns":["secrets/","*.pem","docs/"]}}}"#,
            Level::Floor,
        );
        let reasons = denied_paths(
            &p,
            &["src/a.rs", "secrets/api.txt", "secrets/b.txt", "k/x.pem"],
            AGENT,
        );
        assert_eq!(reasons.len(), 2);
        assert_eq!(reasons[0].params[0].value.raw(), "secrets/api.txt");
        assert_eq!(reasons[0].params[1].value.raw(), "secrets/");
        assert_eq!(reasons[1].params[0].value.raw(), "k/x.pem");
        assert!(denied_paths(&p, &["src/a.rs"], AGENT).is_empty());
        assert!(denied_paths(&p, &["secrets/api.txt"], None).is_empty());
    }

    #[test]
    fn what_cannot_be_read_denies_only_when_a_rule_governs() {
        let p = rules(
            r#"{"policies":{"forbiddenPaths":{"patterns":["secrets/"]}}}"#,
            Level::Floor,
        );
        let touched = Touched {
            paths: Vec::new(),
            unverifiable: true,
        };
        let mut out = Evaluation::allow();
        forbidden_paths(&mut out, &touched, AGENT, &p, &mut Budget::default());
        assert_eq!(out.effect, Effect::Deny);
        assert_eq!(out.reasons[0].cause, Some(Cause::Unverifiable));
        // The person is not governed by an `agents` rule: nothing to verify.
        let mut out = Evaluation::allow();
        forbidden_paths(&mut out, &touched, None, &p, &mut Budget::default());
        assert_eq!(out.effect, Effect::Allow);
        assert!(!p.needs_paths(None));
        assert!(p.needs_paths(AGENT));
    }

    #[test]
    fn running_out_of_work_never_reads_as_no_match() {
        let p = rules(
            r#"{"policies":{"forbiddenPaths":{"patterns":["secrets/"]}}}"#,
            Level::Floor,
        );
        let touched = Touched {
            paths: vec!["a/b/c/d.txt".into()],
            unverifiable: false,
        };
        let mut out = Evaluation::allow();
        forbidden_paths(&mut out, &touched, AGENT, &p, &mut Budget::new(1));
        assert_eq!(out.effect, Effect::Deny);
        assert_eq!(out.reasons[0].cause, Some(Cause::Unverifiable));
    }

    #[test]
    fn degraded_mode_keeps_only_the_rules_that_need_no_actor() {
        let p = rules(
            r#"{"policies":{"protectedBranches":{"patterns":["main"]},
                            "forbiddenPaths":{"patterns":["secrets/"],"appliesTo":"everyone"}}}"#,
            Level::Floor,
        );
        let degraded = p.everyone_only();
        assert!(degraded.branches.is_empty());
        assert_eq!(degraded.paths.len(), 1);
        assert!(degraded.needs_paths(None));
    }
}
