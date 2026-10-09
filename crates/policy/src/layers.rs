//! The personal levels over the team's (BR-CONS-001): the profile's `settings.json` and the
//! repo's `settings.local.json` harden the team rules and never relax them.
//!
//! Pure: no I/O. [`harden`] is the value in force; [`ignored_relaxations`] only observes what a
//! level that cannot relax tried to relax, and never changes a decision.

use gitraptor_api::guard::Level;

use crate::guard::authorship::{self, Policy};
use crate::settings::diagnostic::{Code, Location, SourceKind};
use crate::settings::document::Parsed;
use crate::settings::model::{CommitAuthorship, Operation, Settings};
use crate::team::{EffectivePermissions, OperationRule, Permission, RuleSource, TeamConfig};

/// The two personal levels as read (BR-CONS-001): the profile's `settings.json` and the repo's
/// `settings.local.json`. `None` = absent or ignored.
#[derive(Debug, Clone, Copy, Default)]
pub struct Personal<'a> {
    pub profile: Option<&'a Settings>,
    pub local: Option<&'a Settings>,
}

/// The permission `settings` declares for `op`: the most restrictive of its lists that names
/// it; `None` when none does.
pub fn declared(settings: &Settings, op: Operation) -> Option<Permission> {
    let p = settings.permissions.as_ref()?;
    // Most restrictive first.
    [
        (&p.deny, Permission::Deny),
        (&p.ask, Permission::Ask),
        (&p.allow, Permission::Allow),
    ]
    .into_iter()
    .find(|(list, _)| list.iter().flatten().any(|named| *named == op))
    .map(|(_, permission)| permission)
}

/// The personal value of `op` and the level that sets it: the local one when the local
/// declares it, else the profile one.
fn personal_value(
    op: Operation,
    profile: Option<&Settings>,
    local: Option<&Settings>,
) -> Option<(Permission, Level)> {
    local
        .and_then(|s| declared(s, op))
        .map(|p| (p, Level::Local))
        .or_else(|| {
            profile
                .and_then(|s| declared(s, op))
                .map(|p| (p, Level::Profile))
        })
}

/// The team permissions hardened by the personal levels: per operation the personal value is
/// the local one when the local declares it, else the profile one; the effective one is the
/// maximum of the team and that value. Never below `team`. Sources: the team's, plus
/// `RuleSource::Source(SourceKind::Local | Profile)` when the personal value is the maximum.
pub fn harden(team: &EffectivePermissions, personal: Personal<'_>) -> EffectivePermissions {
    let mut out = team.clone();
    for op in Operation::ALL {
        let Some((value, level)) = personal_value(op, personal.profile, personal.local) else {
            continue;
        };
        let kind = match level {
            Level::Local => SourceKind::Local,
            _ => SourceKind::Profile,
        };
        let rule = out.operations.entry(op).or_insert_with(|| OperationRule {
            permission: Permission::Allow,
            sources: Vec::new(),
        });
        if value > rule.permission {
            rule.permission = value;
            rule.sources = vec![RuleSource::Source(kind)];
        } else if value == rule.permission && value != Permission::Allow {
            // The same effect from one more rule: every rule that produces it is named.
            rule.sources.push(RuleSource::Source(kind));
            rule.sources.sort();
            rule.sources.dedup();
        }
    }
    out
}

/// What a level that only hardens tried to relax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelaxKey {
    Permission(Operation),
    SafeMinimum,
    BaseBranch,
    CommitAuthorship,
}

/// One relaxation that was ignored, and the level that declared it (`Worktree`, `Profile` or
/// `Local`). It says what was declared, never who wrote it: the level's file is not attributed.
// `api::guard::Level` has no `Ord`, so the sort is done by `rank` below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IgnoredRelaxation {
    pub level: Level,
    pub key: RelaxKey,
}

const FLOOR_ONLY_BASE: &str = "/engine/baseBranch";
const FLOOR_ONLY_MINIMUM: &str = "/permissions/disableSafeMinimum";

fn rank(level: Level) -> u8 {
    match level {
        Level::Worktree => 0,
        Level::Profile => 1,
        Level::Local => 2,
        // Never produced here; kept total.
        Level::Minimum | Level::Floor | Level::System => 3,
    }
}

/// Whether the parser dropped the key at `pointer` from `parsed` for being out of its level.
fn dropped(parsed: &Parsed, pointer: &str) -> bool {
    parsed.diagnostics.iter().any(|d| {
        d.code == Code::KeyNotAllowedAtLevel
            && matches!(&d.location, Some(Location::Pointer(p)) if p == pointer)
    })
}

fn authorship_of(settings: Option<&Settings>) -> Option<&CommitAuthorship> {
    settings?.policies.as_ref()?.commit_authorship.as_ref()
}

/// Every relaxation the worktree and the personal level in force declared against the team
/// and that was ignored. Sorted and deduplicated. Pure: never changes a decision.
/// `floor_may_relax`: the floor is the confirmed one and fully readable.
///
/// Counted: a permission below the team's, `disableSafeMinimum` or `engine.baseBranch` outside
/// the floor, and a `commitAuthorship` relaxation the team did not already grant. Not counted:
/// a local that relaxes the profile without going below the team, the profile value the local
/// covers, and empty lists. A key the parser dropped for its level carries no value, so a
/// dropped `disableSafeMinimum: false` is reported too.
pub fn ignored_relaxations(
    team: &TeamConfig,
    profile: &Parsed,
    local: &Parsed,
    floor_may_relax: bool,
) -> Vec<IgnoredRelaxation> {
    let worktree = team.worktree.parsed.applicable();
    let (profile_s, local_s) = (profile.applicable(), local.applicable());
    let mut out = Vec::new();
    let mut push = |level, key| out.push(IgnoredRelaxation { level, key });

    for op in Operation::ALL {
        let team_value = team
            .permissions
            .operations
            .get(&op)
            .map_or(Permission::Allow, |r| r.permission);
        if worktree
            .and_then(|s| declared(s, op))
            .is_some_and(|d| d < team_value)
        {
            push(Level::Worktree, RelaxKey::Permission(op));
        }
        if let Some((value, level)) = personal_value(op, profile_s, local_s)
            && value < team_value
        {
            push(level, RelaxKey::Permission(op));
        }
    }

    // The worktree's keys only the floor may set: the diagnostic the team loader leaves is the
    // single criterion for "different from the floor".
    for d in team.diagnostics() {
        if d.code != Code::FloorOnlyKey || d.source != SourceKind::Worktree {
            continue;
        }
        match &d.location {
            Some(Location::Pointer(p)) if p == FLOOR_ONLY_MINIMUM => {
                push(Level::Worktree, RelaxKey::SafeMinimum);
            }
            Some(Location::Pointer(p)) if p == FLOOR_ONLY_BASE => {
                push(Level::Worktree, RelaxKey::BaseBranch);
            }
            _ => {}
        }
    }
    for (pointer, key) in [
        (FLOOR_ONLY_MINIMUM, RelaxKey::SafeMinimum),
        (FLOOR_ONLY_BASE, RelaxKey::BaseBranch),
    ] {
        // Local before profile, as everywhere else.
        if dropped(local, pointer) {
            push(Level::Local, key);
        } else if dropped(profile, pointer) {
            push(Level::Profile, key);
        }
    }

    // `commitAuthorship`: mirrors the sources the daemon combines. A `flexible` is a relaxation
    // only when the team's own value is not already `flexible`.
    let floor_source = authorship::Source {
        level: Level::Floor,
        setting: authorship_of(team.floor.parsed.applicable()),
        may_relax: floor_may_relax,
    };
    let worktree_source = authorship::Source {
        level: Level::Worktree,
        setting: authorship_of(worktree),
        may_relax: false,
    };
    let (personal_level, personal_setting) = match authorship_of(local_s) {
        Some(s) => (Level::Local, Some(s)),
        None => (Level::Profile, authorship_of(profile_s)),
    };
    let personal_source = authorship::Source {
        level: personal_level,
        setting: personal_setting,
        may_relax: false,
    };
    let floor_policy = authorship::combine(&[floor_source]).effective.policy;
    let team_policy = authorship::combine(&[floor_source, worktree_source])
        .effective
        .policy;
    let all = authorship::combine(&[floor_source, worktree_source, personal_source]);
    for (level, code) in all.diagnostics {
        if code != Code::RelaxationNotAllowed {
            continue;
        }
        let against = match level {
            Level::Worktree => floor_policy,
            Level::Profile | Level::Local => team_policy,
            _ => continue,
        };
        if against != Policy::Flexible {
            push(level, RelaxKey::CommitAuthorship);
        }
    }

    out.sort_by_key(|r| (rank(r.level), r.key));
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::document::parse_document;
    use crate::settings::model::Level as DocLevel;

    fn settings(json: &str) -> Settings {
        // Written at the team level so every key is admitted: the layer is not the subject.
        let parsed = parse_document(json.as_bytes(), DocLevel::Team, SourceKind::Floor);
        parsed.applicable().cloned().expect("valid test document")
    }

    fn team_perms(json: &str) -> EffectivePermissions {
        let s = settings(json);
        crate::team::combine(Some((SourceKind::Floor, &s)), true, &[])
    }

    #[test]
    fn declared_takes_the_most_restrictive_list_that_names_the_operation() {
        let s = settings(
            r#"{"permissions":{"allow":["push","merge"],"ask":["push"],"deny":["push"]}}"#,
        );
        assert_eq!(declared(&s, Operation::Push), Some(Permission::Deny));
        assert_eq!(declared(&s, Operation::Merge), Some(Permission::Allow));
        assert_eq!(declared(&s, Operation::Rebase), None);
        let s = settings(r#"{"permissions":{"allow":["push"],"ask":["push"]}}"#);
        assert_eq!(declared(&s, Operation::Push), Some(Permission::Ask));
        assert_eq!(declared(&Settings::default(), Operation::Push), None);
    }

    #[test]
    fn harden_follows_the_precedence_rows() {
        let team = team_perms(r#"{"permissions":{"deny":["push"],"ask":["rebase"]}}"#);
        let allow = settings(r#"{"permissions":{"allow":["push","rebase"]}}"#);
        let deny = settings(r#"{"permissions":{"deny":["push","merge"]}}"#);

        // Local allow never lowers a team deny / ask.
        let h = harden(
            &team,
            Personal {
                profile: None,
                local: Some(&allow),
            },
        );
        assert_eq!(h.permission(Operation::Push), Permission::Deny);
        assert_eq!(h.permission(Operation::Rebase), Permission::Ask);

        // A personal deny raises, and names its level.
        let h = harden(
            &team,
            Personal {
                profile: Some(&deny),
                local: None,
            },
        );
        assert_eq!(h.permission(Operation::Merge), Permission::Deny);
        assert_eq!(
            h.operations[&Operation::Merge].sources,
            [RuleSource::Source(SourceKind::Profile)]
        );
        // Same effect as the team: both rules are named.
        assert_eq!(
            h.operations[&Operation::Push].sources,
            [
                RuleSource::Source(SourceKind::Profile),
                RuleSource::Source(SourceKind::Floor)
            ]
        );

        // Local over profile (row 3): a local allow covers the profile's deny when the team
        // allows.
        let open_team = team_perms("{}");
        let h = harden(
            &open_team,
            Personal {
                profile: Some(&deny),
                local: Some(&allow),
            },
        );
        assert_eq!(h.permission(Operation::Push), Permission::Allow);
        assert_eq!(h.permission(Operation::Merge), Permission::Deny);
        assert!(h.safe_minimum_active == open_team.safe_minimum_active);
    }

    /// ADR-GRD-003 validation 3: for every team permission and every personal declaration the
    /// effective value is never below the team's, and a personal value above the team's wins.
    #[test]
    fn the_effective_permission_is_never_below_the_team() {
        let perms = [
            None,
            Some(Permission::Allow),
            Some(Permission::Ask),
            Some(Permission::Deny),
        ];
        let doc = |p: Option<Permission>| -> Settings {
            let list = match p {
                None => return Settings::default(),
                Some(Permission::Allow) => "allow",
                Some(Permission::Ask) => "ask",
                Some(Permission::Deny) => "deny",
            };
            settings(&format!(r#"{{"permissions":{{"{list}":["rebase"]}}}}"#))
        };
        for team_p in perms {
            let team = team_perms(&match team_p {
                None => "{}".to_owned(),
                Some(p) => doc_json(p),
            });
            let team_value = team.permission(Operation::Rebase);
            for profile_p in perms {
                for local_p in perms {
                    let (profile, local) = (doc(profile_p), doc(local_p));
                    let h = harden(
                        &team,
                        Personal {
                            profile: Some(&profile),
                            local: Some(&local),
                        },
                    );
                    let effective = h.permission(Operation::Rebase);
                    assert!(
                        effective >= team_value,
                        "{team_p:?} {profile_p:?} {local_p:?}"
                    );
                    let personal = local_p.or(profile_p);
                    assert_eq!(
                        effective,
                        personal.map_or(team_value, |p| p.max(team_value))
                    );
                    // Nothing else moves.
                    for op in Operation::ALL
                        .into_iter()
                        .filter(|o| *o != Operation::Rebase)
                    {
                        assert_eq!(h.permission(op), team.permission(op));
                    }
                    assert_eq!(h.safe_minimum_active, team.safe_minimum_active);
                }
            }
        }
    }

    fn doc_json(p: Permission) -> String {
        let list = match p {
            Permission::Allow => "allow",
            Permission::Ask => "ask",
            Permission::Deny => "deny",
        };
        format!(r#"{{"permissions":{{"{list}":["rebase"]}}}}"#)
    }
}
