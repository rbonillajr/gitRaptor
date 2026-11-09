//! `raptor undo` (US-TMC-002): undoes the last operation of the worktree the
//! command runs in. The engine decides everything (target, permissions,
//! preconditions); the CLI only names the worktree and renders the answer
//! in the user's language.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{
    MAX_SINCE_SECS, NotRestoredReason, TmConfirmData, TmRejectReason, UndoResult, parse_since,
};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::ClientError;
use gitraptor_core::timemachine::overlap::local_utc_offset_s;
use gitraptor_core::timemachine::restore::KEPT_REF_IN_RECREATED_WORKTREE;
use serde_json::json;

pub(crate) mod confirm;
pub(crate) mod notices;
mod overlap;
pub(crate) mod redo;

#[cfg(test)]
#[path = "since_tests.rs"]
mod since_tests;

#[cfg(test)]
#[path = "since_review_tests.rs"]
mod since_review_tests;

#[cfg(test)]
#[allow(
    unused_imports,
    reason = "the frozen test file globs super:: without using it"
)]
#[path = "agent_tests.rs"]
mod agent_tests;

use crate::i18n::{Tr, t, tn};
use crate::{engine, error_text, shown};

const CMD: &str = "raptor undo";

/// The root of the worktree that contains `dir`: the nearest folder with a
/// `.git` entry. The engine checks that it is the root of a worktree of an
/// observed repo.
pub fn worktree_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| std::fs::symlink_metadata(d.join(".git")).is_ok())
        .map(Path::to_path_buf)
}

/// Whose work an undo by agent takes back and since when, as the person reads it.
struct AgentAsk {
    /// The text of `--agent`, as typed.
    agent: String,
    /// Local time of now minus `--since` (or minus the 30 days of retention without it).
    since: String,
}

impl AgentAsk {
    fn new(agent: &str, since: Option<&str>) -> Self {
        Self::at(agent, since, now_ms(), local_utc_offset_s())
    }

    fn at(agent: &str, since: Option<&str>, now_ms: i64, offset_s: i32) -> Self {
        // An invalid duration is the engine's to refuse; the text then never shows the period.
        let secs = since
            .and_then(|s| parse_since(s).ok())
            .unwrap_or(MAX_SINCE_SECS);
        let span_ms = i64::try_from(secs.saturating_mul(1_000)).unwrap_or(i64::MAX);
        Self {
            agent: sanitize(agent),
            since: crate::events::local_time(now_ms.saturating_sub(span_ms), offset_s),
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `raptor undo [--since <duration>] [--agent <id>]`. The duration and the agent go to the
/// engine as typed: the engine validates them and counts the period with its own clock.
pub fn run(json_out: bool, since: Option<&str>, agent: Option<&str>) -> ExitCode {
    let ask = agent.map(|a| AgentAsk::new(a, since));
    let cwd = std::env::current_dir()
        .and_then(gitraptor_cli::paths::canonicalize)
        .ok();
    let Some(worktree) = cwd.as_deref().and_then(worktree_root) else {
        eprintln!("{CMD}: {}", t("undo.not-in-worktree", &[]));
        return ExitCode::FAILURE;
    };
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    notices::show_pending(&mut client, &worktree, CMD);
    let mut params = json!({ "worktree": worktree.to_string_lossy(), "surface": "cli" });
    if let Some(since) = since {
        params["since"] = json!(since);
    }
    if let Some(agent) = agent {
        params["agent"] = json!(agent);
    }
    let answer = match confirm::call(
        &mut client,
        methods::TM_UNDO,
        params,
        !json_out,
        &|data: &TmConfirmData| eprintln!("{CMD}: {}", plan_text(data, since)),
    ) {
        Ok(answer) => answer,
        Err(code) => return code,
    };
    match answer {
        Ok(value) => {
            if json_out {
                println!("{value}");
                return ExitCode::SUCCESS;
            }
            match serde_json::from_value::<UndoResult>(value) {
                Ok(result) => {
                    print_done(&result, &worktree, since, ask.as_ref());
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("{CMD}: {}", sanitize(&err.to_string()));
                    ExitCode::FAILURE
                }
            }
        }
        Err(err) => {
            // `--json` prints an overlap's data as the daemon sent it.
            if json_out
                && let ClientError::Rpc(e) = &err
                && e.code == code::OPERATION_REJECTED
                && let Some(data) = &e.data
                && serde_json::from_value::<TmConfirmData>(data.clone()).is_ok_and(|d| {
                    matches!(
                        d.reason,
                        TmRejectReason::Overlap | TmRejectReason::OverlapUnverified
                    )
                })
            {
                println!("{data}");
                return ExitCode::FAILURE;
            }
            for line in agent_failure_text(err, &worktree, since, ask.as_ref()).lines() {
                eprintln!("{CMD}: {line}");
            }
            ExitCode::FAILURE
        }
    }
}

/// What the confirmation is about: the operation, its id and whose work it is; for a period,
/// how many operations from the oldest, and how many owners when the list is cut.
fn plan_text(data: &TmConfirmData, since: Option<&str>) -> String {
    let operation = data
        .undone_subtype
        .as_ref()
        .map_or_else(|| t("undo.operation-unnamed", &[]), |s| s.sanitized());
    let id = data
        .undone_operation_id
        .as_deref()
        .map(sanitize)
        .unwrap_or_default();
    let mut owners = data
        .owners
        .iter()
        .map(crate::events::actor)
        .collect::<Vec<_>>()
        .join(", ");
    let Some(since) = since else {
        return t(
            "undo.confirm-plan",
            &[("operation", &operation), ("id", &id), ("owners", &owners)],
        );
    };
    let shown = data.owners.len() as u64;
    if data.owners_total > shown {
        let more = t(
            "undo.overlap.more",
            &[("count", &(data.owners_total - shown))],
        );
        owners = if owners.is_empty() {
            more
        } else {
            format!("{owners}, {more}")
        };
    }
    tn(
        "undo.confirm-plan-since",
        data.undone_count.max(1),
        &[
            ("since", &sanitize(since)),
            ("operation", &operation),
            ("id", &id),
            ("owners", &owners),
        ],
    )
}

fn print_done(result: &UndoResult, worktree: &Path, since: Option<&str>, ask: Option<&AgentAsk>) {
    if let Some(ask) = ask {
        // Only the agent's work was taken back: how much, whose, since when, and the prior.
        let count = result.undone_operation_ids.len().max(1).to_string();
        println!(
            "{CMD}: {}",
            t(
                "undo.done-agent",
                &[
                    ("count", &count),
                    ("agent", &ask.agent),
                    ("since", &ask.since),
                    ("worktree", &shown(worktree)),
                    ("id", &sanitize(&result.prior_snapshot_id)),
                ],
            )
        );
    } else if let Some(since) = since {
        // A period may take back one operation or many: the count, the oldest and the prior.
        let count = u64::try_from(result.undone_operation_ids.len().max(1)).unwrap_or(u64::MAX);
        println!(
            "{CMD}: {}",
            tn(
                "undo.done-since",
                count,
                &[
                    ("since", &sanitize(since)),
                    ("worktree", &shown(worktree)),
                    ("oldest", &sanitize(&result.undone_operation_id)),
                    ("id", &sanitize(&result.prior_snapshot_id)),
                ],
            )
        );
    } else {
        let operation = result
            .undone_subtype
            .as_ref()
            .map_or_else(|| t("undo.operation-unnamed", &[]), |s| s.sanitized());
        println!(
            "{CMD}: {}",
            t(
                "undo.done",
                &[
                    ("operation", &operation),
                    ("id", &result.undone_operation_id),
                    ("worktree", &shown(worktree)),
                ],
            )
        );
        println!(
            "{CMD}: {}",
            t("undo.saved", &[("snapshot", &result.prior_snapshot_id)])
        );
    }
    // A branch the undo of a restore left as it is: only a worktree that
    // restore recreated has it out. The ref name is untrusted repo text.
    for kept in result.warnings.iter().filter_map(|w| {
        w.strip_prefix(KEPT_REF_IN_RECREATED_WORKTREE)?
            .strip_prefix(':')
    }) {
        println!("{CMD}: {}", t("undo.kept-ref", &[("ref", &sanitize(kept))]));
    }
    let kept_total = usize::try_from(result.kept_total).unwrap_or(usize::MAX);
    for line in overlap::kept_lines_of(Tr::current(), &result.kept, kept_total) {
        println!("{CMD}: {line}");
    }
    for path in &result.not_restored {
        let reason = t(path_reason_key(path.reason), &[]);
        println!(
            "{CMD}: {}",
            t(
                "undo.not-restored",
                &[("reason", &reason), ("path", &path.path.sanitized())],
            )
        );
    }
}

/// The message for a failed undo: a rejection with its reason, a scope the
/// engine does not observe, an interruption, or the common errors.
#[cfg(test)]
fn failure_text(err: ClientError, worktree: &Path, since: Option<&str>) -> String {
    agent_failure_text(err, worktree, since, None)
}

/// [`failure_text`] for an undo by agent: `ask` names the agent and the period.
fn agent_failure_text(
    err: ClientError,
    worktree: &Path,
    since: Option<&str>,
    ask: Option<&AgentAsk>,
) -> String {
    match err {
        ClientError::Rpc(err)
            if err.code == code::INVALID_PARAMS
                && ask.is_some()
                && err.message.starts_with("agent:") =>
        {
            t("undo.invalid-agent", &[])
        }
        // With --since the period is the only parameter the person typed.
        // The engine names the field first (`since: ...`); another invalid field keeps its text.
        ClientError::Rpc(err)
            if err.code == code::INVALID_PARAMS
                && since.is_some()
                && err.message.starts_with("since:") =>
        {
            t("undo.invalid-since", &[])
        }
        ClientError::Rpc(err) if err.code == code::OPERATION_REJECTED => {
            let data = err
                .data
                .and_then(|d| serde_json::from_value::<TmConfirmData>(d).ok());
            match data {
                Some(d) if d.reason == TmRejectReason::ConfirmationRequired => {
                    t(&confirm::required_key("undo", Some(&d)), &[])
                }
                Some(d)
                    if matches!(
                        d.reason,
                        TmRejectReason::Overlap | TmRejectReason::OverlapUnverified
                    ) =>
                {
                    overlap::rejection_lines(Tr::current(), d.reason, d.overlap.as_ref()).join("\n")
                }
                Some(d) if d.reason == TmRejectReason::NothingToUndo && ask.is_some() => {
                    let (agent, since) = ask.map_or(("", ""), |a| (&a.agent, &a.since));
                    t(
                        "undo.reason.nothing-of-agent",
                        &[("agent", &agent), ("since", &since)],
                    )
                }
                Some(d) if d.reason == TmRejectReason::NotSelectable => t(
                    "undo.reason.not-selectable",
                    &[("agent", &ask.map_or("", |a| a.agent.as_str()))],
                ),
                Some(d) => match (d.reason, since) {
                    (TmRejectReason::NothingToUndo, Some(since)) => t(
                        "undo.reason.nothing-in-period",
                        &[("since", &sanitize(since))],
                    ),
                    (TmRejectReason::RawGitNotCovered, Some(since)) => t(
                        "undo.reason.raw-git-not-covered-since",
                        &[("since", &sanitize(since))],
                    ),
                    (TmRejectReason::TargetUnavailable, Some(since)) => t(
                        "undo.reason.target-unavailable-since",
                        &[("since", &sanitize(since))],
                    ),
                    (reason, _) => t(reason_key(reason), &[]),
                },
                None => t("undo.reason.unsupported", &[]),
            }
        }
        ClientError::Rpc(err) if err.code == code::SCOPE_REFUSED => {
            t("undo.not-observed", &[("worktree", &shown(worktree))])
        }
        ClientError::Rpc(err) if err.code == code::OPERATION_FAILED => {
            let id = err
                .data
                .as_ref()
                .and_then(|d| d.get("operation_id"))
                .and_then(|v| v.as_str())
                .map(sanitize)
                .unwrap_or_default();
            t("undo.interrupted", &[("id", &id)])
        }
        other => error_text(other),
    }
}

fn reason_key(reason: TmRejectReason) -> &'static str {
    match reason {
        TmRejectReason::NothingToUndo => "undo.reason.nothing-to-undo",
        TmRejectReason::RawGitNotCovered => "undo.reason.raw-git-not-covered",
        TmRejectReason::TargetUnavailable => "undo.reason.target-unavailable",
        TmRejectReason::OtherActor => "undo.reason.other-actor",
        TmRejectReason::ConfirmationRequired => "undo.reason.confirmation-required",
        TmRejectReason::GitOperationInProgress => "undo.reason.git-operation-in-progress",
        TmRejectReason::GitBusy => "undo.reason.git-busy",
        TmRejectReason::RepoBusy => "undo.reason.repo-busy",
        TmRejectReason::RepoUntrusted => "undo.reason.repo-untrusted",
        TmRejectReason::WorktreeUnavailable => "undo.reason.worktree-unavailable",
        TmRejectReason::InvalidSnapshot => "undo.reason.invalid-snapshot",
        TmRejectReason::HostileTree => "undo.reason.hostile-tree",
        TmRejectReason::RefMoved => "undo.reason.ref-moved",
        TmRejectReason::RefInUse => "undo.reason.ref-in-use",
        TmRejectReason::Unsupported => "undo.reason.unsupported",
        TmRejectReason::GitUnavailable => "undo.reason.git-unavailable",
        TmRejectReason::ChallengeInvalid => "undo.reason.challenge-invalid",
        TmRejectReason::ConfirmationUnavailable => "undo.reason.confirmation-unavailable-windows",
        TmRejectReason::Overlap => "undo.reason.overlap",
        TmRejectReason::OverlapUnverified => "undo.reason.overlap-unverified",
        TmRejectReason::NotSelectable => "undo.reason.not-selectable",
        TmRejectReason::SelectiveUndoBelow => "undo.reason.selective-undo-below",
        TmRejectReason::AttributionUnavailable => "undo.reason.attribution-unavailable",
        // Only a redo rejects with these; an undo never does.
        TmRejectReason::NothingToRedo | TmRejectReason::RedoInvalidated => {
            "undo.reason.unsupported"
        }
    }
}

fn path_reason_key(reason: NotRestoredReason) -> &'static str {
    match reason {
        NotRestoredReason::Overlap => "undo.path.overlap",
        NotRestoredReason::NotGuaranteed => "undo.path.not-guaranteed",
        NotRestoredReason::Blocked => "undo.path.blocked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::has_key;

    /// Every rejection reason has its message in English and Spanish.
    #[test]
    fn every_rejection_has_a_message() {
        for reason in [
            TmRejectReason::NothingToUndo,
            TmRejectReason::RawGitNotCovered,
            TmRejectReason::TargetUnavailable,
            TmRejectReason::OtherActor,
            TmRejectReason::ConfirmationRequired,
            TmRejectReason::GitOperationInProgress,
            TmRejectReason::GitBusy,
            TmRejectReason::RepoBusy,
            TmRejectReason::RepoUntrusted,
            TmRejectReason::WorktreeUnavailable,
            TmRejectReason::InvalidSnapshot,
            TmRejectReason::HostileTree,
            TmRejectReason::RefMoved,
            TmRejectReason::RefInUse,
            TmRejectReason::Unsupported,
            TmRejectReason::GitUnavailable,
            TmRejectReason::ChallengeInvalid,
            TmRejectReason::ConfirmationUnavailable,
            TmRejectReason::Overlap,
            TmRejectReason::OverlapUnverified,
            TmRejectReason::NotSelectable,
            TmRejectReason::SelectiveUndoBelow,
            TmRejectReason::AttributionUnavailable,
        ] {
            let key = reason_key(reason);
            assert!(has_key(key), "{key}");
            // The key is the wire code: one place to keep in step. The one exception is the
            // rule that forbids confirming (BR-TMC-AUTH-001), told as the Windows case.
            let wire = serde_json::to_value(reason).unwrap();
            if reason != TmRejectReason::ConfirmationUnavailable {
                assert_eq!(key, format!("undo.reason.{}", wire.as_str().unwrap()));
            }
        }
        for reason in [
            NotRestoredReason::Overlap,
            NotRestoredReason::NotGuaranteed,
            NotRestoredReason::Blocked,
        ] {
            assert!(has_key(path_reason_key(reason)));
        }
        for key in [
            "undo.done",
            "undo.saved",
            "undo.not-restored",
            "undo.kept-ref",
            "undo.operation-unnamed",
            "undo.not-in-worktree",
            "undo.not-observed",
            "undo.interrupted",
            "undo.done-agent",
            "undo.reason.nothing-of-agent",
            "undo.invalid-agent",
        ] {
            assert!(has_key(key), "{key}");
        }
    }

    /// The period of an undo by agent is local now minus `--since`, or minus 30 days.
    #[test]
    fn the_agent_period_is_counted_back_from_now() {
        let day = 86_400_000;
        let ask = AgentAsk::at("codex\u{1b}-1", Some("1d"), 40 * day, 3_600);
        assert_eq!(ask.since, "1970-02-09 01:00:00");
        assert!(!ask.agent.contains('\u{1b}'), "{}", ask.agent);
        let ask = AgentAsk::at("codex-1", None, 40 * day, 0);
        assert_eq!(ask.since, "1970-01-11 00:00:00");
    }

    #[test]
    fn the_worktree_is_found_from_a_subfolder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("wt");
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::write(root.join(".git"), "gitdir: /elsewhere\n").unwrap();
        assert_eq!(worktree_root(&root.join("src/deep")), Some(root.clone()));
        assert_eq!(worktree_root(&root), Some(root));
        assert_eq!(worktree_root(tmp.path()), None);
    }
}
