//! Commit authorship rules (US-GRD-018, DS-US-GRD-018 § 5.1) and the combination of the levels
//! that set `policies.commitAuthorship` (D2, BR-CONS-001, Q-GRD-20).
//!
//! The actor enters the condition (D4): the rules only harden, and only when the actor is an
//! agent. With "unattributed" no authorship rule denies or warns (BR-EDGE-004).

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{
    AuthorshipFacts, Cause, CommitStage, Effect, Level, Param, ParamKind, Reason, Rule,
};

use super::Evaluation;
use crate::authorship::agents;
use crate::settings::diagnostic::Code;
use crate::settings::model::{AuthorshipMode, CommitAuthorship, OnAgentCommit};

/// The effective policy, ordered: `flexible < agents-commit < human-author+warn <
/// human-author+deny` (D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Policy {
    Flexible,
    AgentsCommit,
    HumanAuthorWarn,
    HumanAuthorDeny,
}

impl Policy {
    /// What one source sets; `None` when it sets nothing (no key, or no `mode`).
    pub fn of(setting: &CommitAuthorship) -> Option<Self> {
        Some(match setting.mode? {
            AuthorshipMode::Flexible => Self::Flexible,
            AuthorshipMode::AgentsCommit => Self::AgentsCommit,
            AuthorshipMode::HumanAuthor => match setting.on_agent_commit {
                Some(OnAgentCommit::Warn) => Self::HumanAuthorWarn,
                Some(OnAgentCommit::Deny) | None => Self::HumanAuthorDeny,
            },
        })
    }

    /// Name of the mode, for the status and the messages.
    pub fn mode(self) -> &'static str {
        match self {
            Self::Flexible => "flexible",
            Self::AgentsCommit => "agents-commit",
            Self::HumanAuthorWarn | Self::HumanAuthorDeny => "human-author",
        }
    }
}

/// The policy in force and the level that sets it (`system` for the default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effective {
    pub policy: Policy,
    pub level: Level,
}

impl Default for Effective {
    /// `agents-commit`, the default of BR-AUTH-005.
    fn default() -> Self {
        Self {
            policy: Policy::AgentsCommit,
            level: Level::System,
        }
    }
}

/// One level that may set the key.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub level: Level,
    pub setting: Option<&'a CommitAuthorship>,
    /// Only the floor, confirmed and fully readable, may relax below the default (D2).
    pub may_relax: bool,
}

/// The combination and what was ignored on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combined {
    pub effective: Effective,
    /// `(level, code)`: `relaxation-not-allowed` or `key-out-of-place`.
    pub diagnostics: Vec<(Level, Code)>,
}

/// The maximum between the default and the sources that set the key (D2). A `flexible` outside
/// a floor that may relax is ignored with `relaxation-not-allowed`.
pub fn combine(sources: &[Source<'_>]) -> Combined {
    let mut effective = Effective::default();
    let mut explicit = false;
    let mut diagnostics = Vec::new();
    for source in sources {
        let Some(setting) = source.setting else {
            continue;
        };
        if setting.on_agent_commit.is_some() && setting.mode != Some(AuthorshipMode::HumanAuthor) {
            diagnostics.push((source.level, Code::KeyOutOfPlace));
        }
        let Some(policy) = Policy::of(setting) else {
            continue;
        };
        if policy == Policy::Flexible && !source.may_relax {
            diagnostics.push((source.level, Code::RelaxationNotAllowed));
            continue;
        }
        let wins = if explicit {
            policy > effective.policy
        } else {
            // The first source that sets the key replaces the default, unless it is lower and
            // may not relax (already filtered above).
            true
        };
        if wins {
            effective = Effective {
                policy,
                level: source.level,
            };
            explicit = true;
        }
    }
    Combined {
        effective,
        diagnostics,
    }
}

fn agent_name(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::ClaudeCode => "claude-code",
        AgentKind::Other => "other",
    }
}

fn reason(rule: Rule, level: Level, cause: Option<Cause>, agent: AgentKind) -> Reason {
    let mut params = vec![Param::new(ParamKind::Agent, agent_name(agent))];
    if rule == Rule::AuthorshipTrailerRequired
        && let Some(row) = agents::row(agent)
    {
        params.push(Param::new(ParamKind::Example, row.example));
    }
    Reason {
        rule,
        level,
        cause,
        params,
    }
}

/// The authorship rules of one commit (§ 5.1). `actor` is the agent the daemon resolved, `None`
/// when unattributed.
pub fn evaluate(
    out: &mut Evaluation,
    stage: CommitStage,
    actor: Option<AgentKind>,
    facts: Option<&AuthorshipFacts>,
    effective: Effective,
) {
    let Some(agent) = actor else {
        return;
    };
    let level = effective.level;
    match effective.policy {
        Policy::Flexible => {}
        Policy::HumanAuthorDeny => out.add(
            Effect::Deny,
            reason(Rule::AuthorshipHumanAuthor, level, None, agent),
        ),
        // Before the message only `human-author` with `deny` cuts (D6).
        _ if stage == CommitStage::PreCommit => {}
        policy => {
            let unreadable = facts.is_none_or(|f| f.unreadable);
            let signed = facts.is_some_and(|f| !f.unreadable && f.coauthors.contains(&Some(agent)));
            if !signed {
                // With `warn` the commit must still meet `agents-commit` (BR-AUTH-005).
                out.add(
                    Effect::Deny,
                    reason(
                        Rule::AuthorshipTrailerRequired,
                        level,
                        unreadable.then_some(Cause::MessageUnreadable),
                        agent,
                    ),
                );
            }
            if policy == Policy::HumanAuthorWarn {
                out.notice(reason(Rule::AuthorshipHumanAuthor, level, None, agent));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::guard::Effect;

    fn facts(coauthors: Vec<Option<AgentKind>>) -> AuthorshipFacts {
        AuthorshipFacts {
            coauthors,
            trailer_table: 1,
            unreadable: false,
        }
    }

    fn eff(policy: Policy) -> Effective {
        Effective {
            policy,
            level: Level::Floor,
        }
    }

    fn run(
        policy: Policy,
        actor: Option<AgentKind>,
        f: Option<&AuthorshipFacts>,
    ) -> (Effect, Vec<Rule>, Vec<Rule>) {
        let mut out = Evaluation::allow();
        evaluate(&mut out, CommitStage::CommitMsg, actor, f, eff(policy));
        (
            out.effect,
            out.reasons.iter().map(|r| r.rule).collect(),
            out.notices.iter().map(|r| r.rule).collect(),
        )
    }

    const CC: Option<AgentKind> = Some(AgentKind::ClaudeCode);

    /// § 5.1, the whole table.
    #[test]
    fn rules_table() {
        let signed = facts(vec![None, CC]);
        let unsigned = facts(vec![None]);
        let other = facts(vec![Some(AgentKind::Other)]);
        use Policy::*;
        let (allow, deny) = (Effect::Allow, Effect::Deny);
        let tr = Rule::AuthorshipTrailerRequired;
        let ha = Rule::AuthorshipHumanAuthor;
        type Case<'a> = (
            Policy,
            Option<AgentKind>,
            &'a AuthorshipFacts,
            Effect,
            Vec<Rule>,
            Vec<Rule>,
        );
        let cases: Vec<Case<'_>> = vec![
            (AgentsCommit, CC, &signed, allow, vec![], vec![]),
            (AgentsCommit, CC, &unsigned, deny, vec![tr], vec![]),
            // Another agent's trailer does not count.
            (AgentsCommit, CC, &other, deny, vec![tr], vec![]),
            (AgentsCommit, None, &unsigned, allow, vec![], vec![]),
            (HumanAuthorDeny, CC, &signed, deny, vec![ha], vec![]),
            (HumanAuthorDeny, CC, &unsigned, deny, vec![ha], vec![]),
            (HumanAuthorDeny, None, &unsigned, allow, vec![], vec![]),
            (HumanAuthorWarn, CC, &signed, allow, vec![], vec![ha]),
            (HumanAuthorWarn, CC, &unsigned, deny, vec![tr], vec![ha]),
            (HumanAuthorWarn, None, &unsigned, allow, vec![], vec![]),
            (Flexible, CC, &unsigned, allow, vec![], vec![]),
            (Flexible, CC, &signed, allow, vec![], vec![]),
            (Flexible, None, &unsigned, allow, vec![], vec![]),
        ];
        for (policy, actor, f, effect, reasons, notices) in cases {
            assert_eq!(
                run(policy, actor, Some(f)),
                (effect, reasons, notices),
                "{policy:?} {actor:?} {f:?}"
            );
        }
    }

    #[test]
    fn the_deny_names_the_agent_and_the_example() {
        let mut out = Evaluation::allow();
        evaluate(
            &mut out,
            CommitStage::CommitMsg,
            CC,
            Some(&facts(vec![])),
            Effective::default(),
        );
        let r = &out.reasons[0];
        assert_eq!(r.level, Level::System);
        assert_eq!(r.cause, None);
        let values: Vec<_> = r.params.iter().map(|p| p.value.raw()).collect();
        assert_eq!(
            values,
            [
                "claude-code",
                "Co-Authored-By: Claude <noreply@anthropic.com>"
            ]
        );
    }

    #[test]
    fn an_unreadable_message_denies_an_agent_with_its_cause() {
        let mut f = facts(vec![CC]);
        f.unreadable = true;
        let mut out = Evaluation::allow();
        evaluate(
            &mut out,
            CommitStage::CommitMsg,
            CC,
            Some(&f),
            Effective::default(),
        );
        assert_eq!(out.effect, Effect::Deny);
        assert_eq!(out.reasons[0].cause, Some(Cause::MessageUnreadable));
        // Unattributed: nothing, even unreadable.
        let mut out = Evaluation::allow();
        evaluate(
            &mut out,
            CommitStage::CommitMsg,
            None,
            Some(&f),
            Effective::default(),
        );
        assert_eq!(out.effect, Effect::Allow);
    }

    #[test]
    fn pre_commit_only_cuts_human_author_deny() {
        for (policy, effect) in [
            (Policy::AgentsCommit, Effect::Allow),
            (Policy::HumanAuthorWarn, Effect::Allow),
            (Policy::HumanAuthorDeny, Effect::Deny),
        ] {
            let mut out = Evaluation::allow();
            evaluate(&mut out, CommitStage::PreCommit, CC, None, eff(policy));
            assert_eq!(out.effect, effect, "{policy:?}");
            assert!(out.notices.is_empty());
        }
    }

    fn setting(mode: AuthorshipMode, on: Option<OnAgentCommit>) -> CommitAuthorship {
        CommitAuthorship {
            mode: Some(mode),
            on_agent_commit: on,
        }
    }

    fn src(level: Level, s: &CommitAuthorship, may_relax: bool) -> Source<'_> {
        Source {
            level,
            setting: Some(s),
            may_relax,
        }
    }

    #[test]
    fn combination_takes_the_maximum_and_only_the_floor_relaxes() {
        let flexible = setting(AuthorshipMode::Flexible, None);
        let human = setting(AuthorshipMode::HumanAuthor, None);
        let warn = setting(AuthorshipMode::HumanAuthor, Some(OnAgentCommit::Warn));

        // Nothing set: the default.
        assert_eq!(combine(&[]).effective, Effective::default());

        // A confirmed floor relaxes.
        let c = combine(&[src(Level::Floor, &flexible, true)]);
        assert_eq!(c.effective.policy, Policy::Flexible);
        assert!(c.diagnostics.is_empty());

        // A personal level does not.
        for level in [Level::Worktree, Level::Profile, Level::Local] {
            let c = combine(&[src(level, &flexible, false)]);
            assert_eq!(c.effective, Effective::default(), "{level:?}");
            assert_eq!(c.diagnostics, [(level, Code::RelaxationNotAllowed)]);
        }
        // An unconfirmed floor does not either.
        let c = combine(&[src(Level::Floor, &flexible, false)]);
        assert_eq!(c.effective.policy, Policy::AgentsCommit);

        // A local flexible does not relax the team's human-author.
        let c = combine(&[
            src(Level::Floor, &human, true),
            src(Level::Local, &flexible, false),
        ]);
        assert_eq!(
            c.effective,
            Effective {
                policy: Policy::HumanAuthorDeny,
                level: Level::Floor
            }
        );

        // Hardening from any level wins.
        let c = combine(&[
            src(Level::Floor, &warn, true),
            src(Level::Profile, &human, false),
        ]);
        assert_eq!(c.effective.policy, Policy::HumanAuthorDeny);
        assert_eq!(c.effective.level, Level::Profile);

        // A floor flexible plus a profile human-author+warn: the warn.
        let c = combine(&[
            src(Level::Floor, &flexible, true),
            src(Level::Profile, &warn, false),
        ]);
        assert_eq!(c.effective.policy, Policy::HumanAuthorWarn);
    }

    #[test]
    fn on_agent_commit_out_of_place_is_ignored() {
        let s = setting(AuthorshipMode::AgentsCommit, Some(OnAgentCommit::Warn));
        let c = combine(&[src(Level::Floor, &s, true)]);
        assert_eq!(c.effective.policy, Policy::AgentsCommit);
        assert_eq!(c.diagnostics, [(Level::Floor, Code::KeyOutOfPlace)]);
    }

    /// D2 as a property: whatever the personal levels say, the result is never below what the
    /// floor alone gives.
    #[test]
    fn a_personal_level_never_lowers_the_maximum() {
        let modes = [
            None,
            Some(setting(AuthorshipMode::Flexible, None)),
            Some(setting(AuthorshipMode::AgentsCommit, None)),
            Some(setting(
                AuthorshipMode::HumanAuthor,
                Some(OnAgentCommit::Warn),
            )),
            Some(setting(AuthorshipMode::HumanAuthor, None)),
        ];
        for floor in &modes {
            for confirmed in [true, false] {
                let floor_src = Source {
                    level: Level::Floor,
                    setting: floor.as_ref(),
                    may_relax: confirmed,
                };
                let alone = combine(&[floor_src]).effective.policy;
                for wt in &modes {
                    for profile in &modes {
                        for local in &modes {
                            fn personal(level: Level, s: &Option<CommitAuthorship>) -> Source<'_> {
                                Source {
                                    level,
                                    setting: s.as_ref(),
                                    may_relax: false,
                                }
                            }
                            let all = combine(&[
                                floor_src,
                                personal(Level::Worktree, wt),
                                personal(Level::Profile, profile),
                                personal(Level::Local, local),
                            ])
                            .effective
                            .policy;
                            assert!(all >= alone, "{floor:?} {wt:?} {profile:?} {local:?}");
                            // Never below the default unless the floor relaxed.
                            if alone != Policy::Flexible {
                                assert!(all >= Policy::AgentsCommit);
                            }
                        }
                    }
                }
            }
        }
    }
}
