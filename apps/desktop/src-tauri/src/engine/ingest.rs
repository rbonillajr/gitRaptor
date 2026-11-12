//! Translation of the engine's contract types into the app's DTOs, sanitizing every text that
//! comes from the file system or an agent before it reaches the webview.

use std::path::Path;

use gitraptor_api::actor::{Actor, AgentKind, AgentOrigin};
use gitraptor_api::clock;
use gitraptor_api::discovery::{CandidateView, RepoDiscoveredData};
use gitraptor_api::display::{NAME_MAX_CHARS, TEXT_MAX_CHARS, visible};
use gitraptor_api::event::{self, Event};
use gitraptor_api::graph::{
    BaseGap, GraphBase, GraphCut, GraphLanes, Lane, LaneCommit, LaneGap, LaneHead, LaneState,
};
use gitraptor_api::messages::{
    BaseStatusView, CommitCountView, DivergenceView, EngineStateView, EngineView, HeadView,
    OperationKindView, RepoObservationData, RepoStateView, RepoTier, RepoTierData,
    SessionEndCauseView, SessionStateView, SessionView, UnavailableReason as UnavailableReasonView,
    WorktreeStateData, WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods;
use gitraptor_api::prediction::{
    ConflictKindView, LimitCode as ApiLimitCode, LineRange, PairDetail, PairLevel as ApiPairLevel,
    PairReason as ApiPairReason, PairSideView, PairStateView, PairSummary, PredictionChanged,
};
use gitraptor_api::scope::{
    AttentionCount, AttentionView, GlobalSnapshot, RepoAttentionData, RepoSnapshot, RepoSummaryView,
};

use gitraptor_api::guard::{
    self, GuardLogEntry, GuardLogResult, GuardStatus, LogDetail, LogKind, LogLayer,
    LoggedOperation, LoggedRef, PendingKind, ProtectionState,
};

use super::exception_command;
use super::shell_command;
use super::wire::{
    AgentDto, AttentionDto, Autostart, BaseDto, BaseStatus, BlockActorDto, BlockDetail,
    BlockEntryDto, BlockKind, BlockLayer, BlockRuleDto, BlockWorktreeDto, BoundedCountDto,
    CandidateDto, CommitDto, ConfirmCommandDto, ConflictFileDto, ConflictFilesDto, ConflictKind,
    CountDto, CountsDto, DaemonDto, DivergenceDto, EndCause, EngineDto, EngineMessage,
    EngineStateDto, ExceptionCommandDto, FleetPerf, GlobalDto, GraphBaseDto, GraphBaseGap,
    GraphCutDto, GraphDto, GuardBlocksDto, GuardSummaryDto, HeadDto, LaneDto, LaneGapDto,
    LaneHeadDto, LaneStateDto, LimitCode, LineRangeDto, OperationDto, PairDetailDto, PairDto,
    PairLevel, PairReason, PairState, PendingExceptionDto, ProtectionDto, ProtectionKind, ReadyDto,
    RefChange, RefDto, RepoFleetDto, RepoStateDto, RepoSummaryDto, SessionDto, SessionOrigin,
    SessionState, Tier, UnavailableReason, UnloggedDto, WorktreeDto, WorktreeStatusDto,
};

/// The global snapshot as a DTO.
pub fn global(snapshot: &GlobalSnapshot) -> GlobalDto {
    GlobalDto {
        engine: engine(&snapshot.engine),
        daemon: DaemonDto {
            version: visible(&snapshot.daemon.binary_version, NAME_MAX_CHARS),
            protocol: snapshot.daemon.protocol,
        },
        autostart: match snapshot.autostart {
            gitraptor_api::scope::AutostartView::Registered => Autostart::Registered,
            gitraptor_api::scope::AutostartView::NotRegistered => Autostart::NotRegistered,
            gitraptor_api::scope::AutostartView::Unknown => Autostart::Unknown,
        },
        repos: snapshot.repos.iter().map(repo_summary).collect(),
    }
}

/// The FNV-1a 64 key of a raw path, as 16 lowercase hex digits.
///
/// It hashes the bytes the engine sent, never the sanitized text, so two different raw paths
/// never share a key because they look alike.
pub fn key(raw_path: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in raw_path.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The open repo's snapshot as a DTO.
///
/// `raptor` is the executable the confirm command names (see [`super::shell_command::raptor_path`]).
pub fn repo(snapshot: &RepoSnapshot, raptor: Option<&Path>) -> RepoFleetDto {
    let view = &snapshot.repo;
    let (pairs, pairs_truncated, limits) = match &view.prediction {
        Some(prediction) => (
            prediction.pairs.iter().map(pair).collect(),
            prediction.pairs_truncated,
            prediction.limits.iter().copied().map(limit).collect(),
        ),
        None => (Vec::new(), false, Vec::new()),
    };
    RepoFleetDto {
        repo_id: visible(&view.repo_id, NAME_MAX_CHARS),
        name: visible(&repo_name(view.path.raw()), NAME_MAX_CHARS),
        base: BaseDto {
            name: view
                .base
                .name
                .as_ref()
                .map(|name| visible(name.raw(), NAME_MAX_CHARS)),
            status: match view.base.status {
                BaseStatusView::Confirmed => BaseStatus::Confirmed,
                BaseStatusView::Unconfirmed => BaseStatus::Unconfirmed,
                BaseStatusView::Invalid => BaseStatus::Invalid,
            },
        },
        fetched_utc_ms: view.fetched_utc_ms.and_then(non_negative),
        tier: view.tier.map(tier),
        worktrees: view.worktrees.iter().map(worktree).collect(),
        pairs,
        pairs_truncated,
        limits,
        confirm_command: confirm_command(view.path.raw(), raptor),
    }
}

/// The command that confirms the base of the repo at `raw_repo_path`, when `raptor` is known.
/// The raw path is quoted, never the sanitized text, so the command works pasted anywhere.
pub fn confirm_command(raw_repo_path: &str, raptor: Option<&Path>) -> ConfirmCommandDto {
    shell_command::confirm(raptor, raw_repo_path)
}

fn limit(code: ApiLimitCode) -> LimitCode {
    match code {
        ApiLimitCode::OverlapIncludesUncommitted => LimitCode::OverlapIncludesUncommitted,
        ApiLimitCode::ConflictCommittedOnly => LimitCode::ConflictCommittedOnly,
        ApiLimitCode::NoMergeDriversOrAttributes => LimitCode::NoMergeDriversOrAttributes,
        ApiLimitCode::RenameLimit => LimitCode::RenameLimit,
        ApiLimitCode::InsertionsMayMerge => LimitCode::InsertionsMayMerge,
        ApiLimitCode::JoinedHunks => LimitCode::JoinedHunks,
        ApiLimitCode::PairIgnoresBaseMoves => LimitCode::PairIgnoresBaseMoves,
        ApiLimitCode::LocalOnly => LimitCode::LocalOnly,
    }
}

/// One pair with its files as a DTO; every path goes through `display::visible`.
pub fn pair_detail(detail: &PairDetail) -> PairDetailDto {
    let path = |p: &gitraptor_api::Untrusted| visible(p.raw(), TEXT_MAX_CHARS);
    let range = |r: &LineRange| LineRangeDto {
        start: r.start,
        len: r.len,
    };
    PairDetailDto {
        pair: pair(&detail.summary),
        overlap: detail.overlap.iter().map(path).collect(),
        conflict: detail.conflict.as_ref().map(|conflict| ConflictFilesDto {
            files: conflict
                .files
                .iter()
                .map(|file| ConflictFileDto {
                    path: path(&file.path),
                    kind: match file.kind {
                        ConflictKindView::Content => ConflictKind::Content,
                        ConflictKindView::AddAdd => ConflictKind::AddAdd,
                        ConflictKindView::ModifyDelete => ConflictKind::ModifyDelete,
                        ConflictKindView::Rename => ConflictKind::Rename,
                        ConflictKindView::DirectoryFile => ConflictKind::DirectoryFile,
                        ConflictKindView::Binary => ConflictKind::Binary,
                        ConflictKindView::Submodule => ConflictKind::Submodule,
                    },
                    ours: file.ours.iter().map(range).collect(),
                    theirs: file.theirs.iter().map(range).collect(),
                    hunks_truncated: file.hunks_truncated,
                    has_merge_rule: file.has_merge_rule,
                })
                .collect(),
            truncated: conflict.truncated,
        }),
    }
}

fn non_negative(ms: i64) -> Option<u64> {
    u64::try_from(ms).ok()
}

/// The last non-empty component of a path, as sent.
fn last_component(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or_default()
}

/// One worktree as a DTO.
pub fn worktree(view: &WorktreeView) -> WorktreeDto {
    let raw = view.path.raw();
    WorktreeDto {
        key: key(raw),
        folder: visible(last_component(raw), NAME_MAX_CHARS),
        path: visible(raw, TEXT_MAX_CHARS),
        main: view.main,
        status: match &view.status {
            WorktreeStatus::Ready {
                head,
                counts,
                divergence: div,
                ..
            } => WorktreeStatusDto::Ready(ReadyDto {
                head: match head {
                    HeadView::Branch { name } => HeadDto::Branch {
                        name: visible(name.raw(), NAME_MAX_CHARS),
                    },
                    HeadView::Unborn { name } => HeadDto::Unborn {
                        name: visible(name.raw(), NAME_MAX_CHARS),
                    },
                    HeadView::Detached => HeadDto::Detached,
                },
                counts: CountsDto {
                    staged: counts.staged,
                    unstaged: counts.unstaged,
                    untracked: counts.untracked,
                },
                divergence: divergence(div),
            }),
            WorktreeStatus::Unavailable { reason } => WorktreeStatusDto::Unavailable {
                reason: match reason {
                    UnavailableReasonView::Missing => UnavailableReason::Missing,
                    UnavailableReasonView::Untrusted => UnavailableReason::Untrusted,
                    UnavailableReasonView::Unreadable => UnavailableReason::Unreadable,
                },
            },
        },
        last_activity_utc_ms: view.last_activity_utc_ms.and_then(non_negative),
        last_activity_in_gap: view.last_activity_in_gap,
        detached_at: view
            .detached_at
            .as_deref()
            .filter(|hash| is_object_id(hash))
            .map(str::to_owned),
        operation: view.operation.map(operation),
    }
}

/// A full object id: 40 (SHA-1) or 64 (SHA-256) hex digits, nothing else.
fn is_object_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn operation(kind: OperationKindView) -> String {
    match kind {
        OperationKindView::Rebase => "rebase",
        OperationKindView::Merge => "merge",
        OperationKindView::CherryPick => "cherry-pick",
        OperationKindView::Revert => "revert",
        OperationKindView::Am => "am",
        OperationKindView::Bisect => "bisect",
    }
    .to_owned()
}

fn bounded(count: CommitCountView) -> BoundedCountDto {
    BoundedCountDto {
        count: u32::try_from(count.count).unwrap_or(u32::MAX),
        exact: count.exact,
    }
}

fn divergence(view: &DivergenceView) -> DivergenceDto {
    match view {
        DivergenceView::Counted { ahead, behind } => DivergenceDto::Counted {
            ahead: bounded(*ahead),
            behind: bounded(*behind),
        },
        DivergenceView::BaseMissing => DivergenceDto::BaseMissing,
        DivergenceView::NoBase => DivergenceDto::NoBase,
        DivergenceView::NoCommits => DivergenceDto::NoCommits,
        DivergenceView::Unreadable => DivergenceDto::Unreadable,
    }
}

/// One session as a DTO; `None` when it cannot be attributed to an agent.
pub fn session(view: &SessionView) -> Option<SessionDto> {
    let Actor::Agent { kind, name, origin } = &view.actor else {
        return None;
    };
    Some(SessionDto {
        session_id: visible(&view.session_id, NAME_MAX_CHARS),
        worktree_key: key(view.worktree.raw()),
        agent: match kind {
            AgentKind::ClaudeCode => AgentDto::ClaudeCode,
            AgentKind::Other => AgentDto::Other {
                name: visible(name.as_ref().map_or("", |n| n.raw()), NAME_MAX_CHARS),
            },
        },
        origin: match origin {
            AgentOrigin::Detected => SessionOrigin::Detected,
            AgentOrigin::Registered => SessionOrigin::Registered,
        },
        state: match view.state {
            SessionStateView::Active => SessionState::Active,
            SessionStateView::Inactive => SessionState::Inactive,
            SessionStateView::Ended => SessionState::Ended,
        },
        started_utc_ms: non_negative(view.started_utc_ms).unwrap_or(0),
        state_since_utc_ms: non_negative(view.state_since_utc_ms).unwrap_or(0),
        ended_utc_ms: view.ended_utc_ms.and_then(non_negative),
        end_cause: view.end_cause.map(|cause| match cause {
            SessionEndCauseView::ProcessGone => EndCause::ProcessGone,
            SessionEndCauseView::EndedDuringGap => EndCause::EndedDuringGap,
            SessionEndCauseView::RegistrationWithdrawn => EndCause::RegistrationWithdrawn,
            SessionEndCauseView::WorktreeRemoved => EndCause::WorktreeRemoved,
        }),
    })
}

fn side(side: &PairSideView) -> String {
    match side {
        PairSideView::Worktree { root } => key(root.raw()),
        PairSideView::Base {} => "base".to_owned(),
    }
}

/// One predicted pair as a DTO.
pub fn pair(summary: &PairSummary) -> PairDto {
    PairDto {
        pair_id: visible(&summary.pair_id, NAME_MAX_CHARS),
        a: side(&summary.a),
        b: side(&summary.b),
        state: match summary.state {
            PairStateView::Calculating => PairState::Calculating,
            PairStateView::Current => PairState::Current,
            PairStateView::Recalculating => PairState::Recalculating,
            PairStateView::Pending => PairState::Pending,
            PairStateView::NotComputable => PairState::NotComputable,
        },
        rev: summary.rev,
        reason: summary.reason.map(|reason| match reason {
            ApiPairReason::BaseUnconfirmed => PairReason::BaseUnconfirmed,
            ApiPairReason::OperationInProgress => PairReason::OperationInProgress,
            ApiPairReason::PowerSaving => PairReason::PowerSaving,
            ApiPairReason::Dormant => PairReason::Dormant,
            ApiPairReason::BaseMissing => PairReason::BaseMissing,
            ApiPairReason::MissingObject => PairReason::MissingObject,
            ApiPairReason::Shallow => PairReason::Shallow,
            ApiPairReason::LimitExceeded => PairReason::LimitExceeded,
            ApiPairReason::Error => PairReason::Error,
        }),
        computed_at_utc_ms: summary.computed_at_utc_ms.and_then(non_negative),
        overlap_files: summary.overlap_files,
        overlap_partial: summary.overlap_partial,
        conflict_files: summary.conflict_files,
        conflict_truncated: summary.conflict_truncated,
        level: summary.level.map(|level| match level {
            ApiPairLevel::Clean => PairLevel::None,
            ApiPairLevel::Overlap => PairLevel::Overlap,
            ApiPairLevel::Conflict => PairLevel::Conflict,
        }),
    }
}

/// The messages a repo-scope event produces, `backend_ns` after `recv_ns` (the monotonic reading
/// of when the link read it); empty for events the app ignores or cannot read.
pub fn repo_event(event: &Event, recv_ns: u64) -> Option<EngineMessage> {
    match event.kind.as_str() {
        event::WORKTREE_STATE => {
            decode::<WorktreeStateData>(event).map(|data| EngineMessage::Worktrees {
                repo_id: visible(&data.repo_id, NAME_MAX_CHARS),
                worktrees: data.worktrees.iter().map(worktree).collect(),
                fetched_utc_ms: data.fetched_utc_ms.and_then(non_negative),
                perf: perf(recv_ns),
            })
        }
        event::SESSION_STATE => decode::<SessionView>(event).and_then(|view| {
            session(&view).map(|session| EngineMessage::RepoSession {
                repo_id: visible(&view.repo_id, NAME_MAX_CHARS),
                session,
                perf: perf(recv_ns),
            })
        }),
        methods::PREDICTION_CHANGED => {
            decode::<PredictionChanged>(event).map(|data| EngineMessage::Prediction {
                repo_id: visible(&data.repo_id, NAME_MAX_CHARS),
                pairs: data.pairs.iter().map(pair).collect(),
                removed: data
                    .removed
                    .iter()
                    .map(|id| visible(id, NAME_MAX_CHARS))
                    .collect(),
                perf: perf(recv_ns),
            })
        }
        _ => None,
    }
}

/// A page of pair details for the window.
pub fn pair_details(
    repo_id: &str,
    details: &[PairDetail],
    filled: bool,
    recv_ns: u64,
) -> EngineMessage {
    EngineMessage::PairDetails {
        repo_id: visible(repo_id, NAME_MAX_CHARS),
        details: details.iter().map(pair_detail).collect(),
        filled,
        perf: perf(recv_ns),
    }
}

/// Time since `recv_ns`, never zero so "no measure" stays distinguishable.
pub fn perf(recv_ns: u64) -> FleetPerf {
    FleetPerf {
        backend_ns: clock::monotonic_ns().saturating_sub(recv_ns).max(1),
    }
}

/// One repo of the global scope as a DTO.
pub fn repo_summary(repo: &RepoSummaryView) -> RepoSummaryDto {
    summary(
        &repo.repo_id,
        repo.state,
        repo.path.raw(),
        repo.tier.map(tier),
        attention(&repo.attention),
    )
}

fn summary(
    repo_id: &str,
    state: RepoStateView,
    path: &str,
    tier: Option<Tier>,
    attention: AttentionDto,
) -> RepoSummaryDto {
    RepoSummaryDto {
        repo_id: visible(repo_id, NAME_MAX_CHARS),
        name: visible(&repo_name(path), NAME_MAX_CHARS),
        path: visible(path, TEXT_MAX_CHARS),
        state: match state {
            RepoStateView::Observed => RepoStateDto::Observed,
            RepoStateView::Unavailable => RepoStateDto::Unavailable,
        },
        tier,
        attention,
    }
}

/// The last component of `path` without a trailing `.git`; the parent's name when the last
/// component is `.git` itself.
fn repo_name(path: &str) -> String {
    let mut parts = path.rsplit(['/', '\\']).filter(|part| !part.is_empty());
    let last = parts.next().unwrap_or_default();
    if last == ".git" {
        return parts.next().unwrap_or_default().to_owned();
    }
    last.strip_suffix(".git").unwrap_or(last).to_owned()
}

fn tier(tier: RepoTier) -> Tier {
    match tier {
        RepoTier::Active => Tier::Active,
        RepoTier::Waking => Tier::Waking,
        RepoTier::Dormant => Tier::Dormant,
    }
}

fn count(count: AttentionCount) -> CountDto {
    match count {
        AttentionCount::Counted { count } => CountDto::Counted { count },
        AttentionCount::Unavailable { .. } => CountDto::Unavailable,
    }
}

fn attention(view: &AttentionView) -> AttentionDto {
    AttentionDto {
        conflicts: count(view.conflicts),
        denials: count(view.denials),
    }
}

/// A discovered repo as a DTO.
pub fn candidate(candidate: &CandidateView) -> CandidateDto {
    CandidateDto {
        name: visible(&candidate.name, NAME_MAX_CHARS),
        path: visible(&candidate.path, TEXT_MAX_CHARS),
    }
}

/// The state of an engine the app only needs the state of (the MCP projection has no Git version).
pub fn engine_state(state: EngineStateView, git_version: Option<&str>) -> EngineDto {
    EngineDto {
        state: match state {
            EngineStateView::WaitingForGit => EngineStateDto::WaitingForGit,
            EngineStateView::NoRepos => EngineStateDto::NoRepos,
            EngineStateView::Observing => EngineStateDto::Observing,
        },
        git_version: git_version.map(|v| visible(v, NAME_MAX_CHARS)),
    }
}

/// The engine part of a snapshot or an `engine.state` event as a DTO.
pub fn engine(engine: &EngineView) -> EngineDto {
    engine_state(engine.state, engine.git_version.as_deref())
}

/// The messages a global-scope event produces; empty for events the app ignores or cannot read.
pub fn global_event(event: &Event) -> Vec<EngineMessage> {
    match event.kind.as_str() {
        event::ENGINE_STATE => decode::<EngineView>(event)
            .map(|view| EngineMessage::EngineState {
                engine: engine(&view),
            })
            .into_iter()
            .collect(),
        event::REPO_OBSERVATION => decode::<RepoObservationData>(event)
            .map(|data| {
                if data.observed {
                    let unknown = AttentionDto {
                        conflicts: CountDto::Unavailable,
                        denials: CountDto::Unavailable,
                    };
                    EngineMessage::RepoObserved {
                        repo: summary(&data.repo_id, data.state, data.path.raw(), None, unknown),
                    }
                } else {
                    EngineMessage::RepoRetired {
                        repo_id: visible(&data.repo_id, NAME_MAX_CHARS),
                    }
                }
            })
            .into_iter()
            .collect(),
        event::REPO_TIER => decode::<RepoTierData>(event)
            .map(|data| EngineMessage::RepoTier {
                repo_id: visible(&data.repo_id, NAME_MAX_CHARS),
                tier: tier(data.tier),
            })
            .into_iter()
            .collect(),
        event::REPO_ATTENTION => decode::<RepoAttentionData>(event)
            .map(|data| EngineMessage::RepoAttention {
                repo_id: visible(&data.repo_id, NAME_MAX_CHARS),
                attention: attention(&data.attention),
            })
            .into_iter()
            .collect(),
        event::REPO_DISCOVERED => decode::<RepoDiscoveredData>(event)
            .map(|data| EngineMessage::Discovered {
                candidates: data.candidates.iter().map(candidate).collect(),
                replace: false,
            })
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn decode<T: serde::de::DeserializeOwned>(event: &Event) -> Option<T> {
    serde_json::from_value(event.data.clone()).ok()
}

/// Longest id of a decision shown.
const DECISION_ID_MAX_CHARS: usize = 128;

/// The guard log of a repo as a DTO: only the entries D4 lists, every text escaped, and for each
/// denial the copyable command or the reason there is none.
///
/// Without a `status` the protection reads as unprotected: the supervisor never builds the list
/// in that case (it reports the log as unavailable instead).
#[must_use]
pub fn guard_blocks(
    repo_id: &str,
    status: Option<&GuardStatus>,
    log: &GuardLogResult,
    raptor: Option<&Path>,
) -> GuardBlocksDto {
    GuardBlocksDto {
        repo_id: visible(repo_id, NAME_MAX_CHARS),
        protection: status.map_or(
            ProtectionDto {
                state: ProtectionKind::Unprotected,
            },
            protection,
        ),
        since_ms: log.since_ms,
        summary: GuardSummaryDto {
            blocked: log.summary.blocked,
            exceptions: log.summary.exceptions,
            exceptions_rejected: log.summary.exceptions_rejected,
        },
        entries: log
            .entries
            .iter()
            .filter_map(|entry| guard_entry(entry, raptor))
            .collect(),
        pending: status.and_then(|s| pending(s, log, raptor)),
        unlogged: log
            .unlogged_periods
            .iter()
            .map(|p| UnloggedDto {
                from_ms: p.from_ms,
                to_ms: p.to_ms,
            })
            .collect(),
    }
}

/// The protection of a `guard.status` answer.
#[must_use]
pub fn protection(status: &GuardStatus) -> ProtectionDto {
    ProtectionDto {
        state: match status.state {
            ProtectionState::Unprotected => ProtectionKind::Unprotected,
            ProtectionState::HooksOnly => ProtectionKind::HooksOnly,
            ProtectionState::McpOnly => ProtectionKind::McpOnly,
            ProtectionState::Full => ProtectionKind::Full,
        },
    }
}

/// The name an enum has on the wire (`minimum.force-push`, `floor`, `not-fast-forward`).
fn wire_name<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => visible(&name, NAME_MAX_CHARS),
        _ => String::new(),
    }
}

fn logged_operation(op: &LoggedOperation) -> Option<OperationDto> {
    let refs = |refs: &[LoggedRef]| -> Vec<RefDto> {
        refs.iter()
            .map(|r| RefDto {
                name: visible(r.name.raw(), NAME_MAX_CHARS),
                change: match r.change {
                    guard::RefChange::Create => RefChange::Create,
                    guard::RefChange::Update => RefChange::Update,
                    guard::RefChange::Force => RefChange::Force,
                    guard::RefChange::Delete => RefChange::Delete,
                },
            })
            .collect()
    };
    match op {
        LoggedOperation::Push { remote, refs: r } => Some(OperationDto::Push {
            remote: remote.as_ref().map(|r| visible(r.raw(), NAME_MAX_CHARS)),
            refs: refs(r),
        }),
        LoggedOperation::RefTransaction { refs: r } => {
            Some(OperationDto::RefTransaction { refs: refs(r) })
        }
        LoggedOperation::Rebase { .. } => Some(OperationDto::Rebase),
        LoggedOperation::Commit { .. } => Some(OperationDto::Commit),
        LoggedOperation::ProtectionState { .. } | LoggedOperation::McpAllowlist { .. } => None,
    }
}

/// One log entry as a DTO; `None` for what D4 does not list (a notice, a protection change).
#[must_use]
pub fn guard_entry(entry: &GuardLogEntry, raptor: Option<&Path>) -> Option<BlockEntryDto> {
    let kind = match entry.kind {
        LogKind::Denial => BlockKind::Denial,
        LogKind::Exception => BlockKind::Exception,
        LogKind::ExceptionCancelled => BlockKind::ExceptionCancelled,
        LogKind::ExceptionRejected => BlockKind::ExceptionRejected,
        LogKind::Notice | LogKind::ProtectionState => return None,
    };
    let operation = logged_operation(&entry.operation)?;
    Some(BlockEntryDto {
        decision_id: visible(&entry.decision_id, DECISION_ID_MAX_CHARS),
        kind,
        at_ms: entry.at_ms,
        last_ms: entry.last_ms,
        count: entry.count,
        actor: entry.actor.map(|actor| match actor {
            AgentKind::ClaudeCode => BlockActorDto::ClaudeCode,
            AgentKind::Other => BlockActorDto::Other,
        }),
        worktree: entry.worktree.as_ref().map(|w| BlockWorktreeDto {
            folder: visible(last_component(w.raw()), NAME_MAX_CHARS),
            path: visible(w.raw(), TEXT_MAX_CHARS),
        }),
        branch: entry
            .branch
            .as_ref()
            .map(|b| visible(b.raw(), NAME_MAX_CHARS)),
        operation,
        rules: entry
            .reasons
            .iter()
            .map(|r| BlockRuleDto {
                rule: wire_name(&r.rule),
                level: wire_name(&r.level),
                cause: r.cause.as_ref().map(wire_name),
            })
            .collect(),
        layer: match entry.layer {
            LogLayer::Hooks => BlockLayer::Hooks,
            LogLayer::Mcp => BlockLayer::Mcp,
            LogLayer::Cockpit => BlockLayer::Cockpit,
        },
        detail: match entry.detail {
            LogDetail::Full => BlockDetail::Full,
            LogDetail::RateLimited => BlockDetail::RateLimited,
        },
        exception: (kind == BlockKind::Denial).then(|| {
            match exception_command::exception(raptor, entry) {
                Ok(command) => ExceptionCommandDto::Offered { command },
                Err(reason) => ExceptionCommandDto::NotOffered { reason },
            }
        }),
    })
}

/// The announced exception of `status`, if any. Its cancel command names the worktree of the
/// latest denial of the same operation; with none, the supervisor adds the repo path.
#[must_use]
pub fn pending(
    status: &GuardStatus,
    log: &GuardLogResult,
    raptor: Option<&Path>,
) -> Option<PendingExceptionDto> {
    let action = status.pending.as_ref()?;
    if action.action != PendingKind::Exception {
        return None;
    }
    let plan = action.exception.as_ref()?;
    let op = logged_operation(&plan.operation)?;
    let cancel_command = log
        .entries
        .iter()
        .filter(|e| {
            e.kind == LogKind::Denial && logged_operation(&e.operation).as_ref() == Some(&op)
        })
        .find_map(|e| e.worktree.as_ref())
        .and_then(|w| exception_command::cancel(raptor, w.raw()));
    Some(PendingExceptionDto {
        applies_at_ms: action.applies_at_ms,
        window_ms: action.window_ms,
        operation: op,
        rules: plan.rules.iter().map(wire_name).collect(),
        cancel_command,
    })
}

/// The lanes of `graph.lanes` as a DTO, every untrusted text sanitized.
///
/// A commit whose id is not a full hex object id, or whose time is negative, is omitted; a lane
/// whose merge-base is not a full hex object id becomes `not-computable` `unreadable`. The raw
/// worktree path never leaves: the lane carries its [`key`].
#[must_use]
pub fn graph(lanes: &GraphLanes) -> GraphDto {
    GraphDto {
        computed_utc_ms: lanes.computed_at_utc_ms,
        base: graph_base(&lanes.base),
        lanes: lanes.lanes.iter().map(lane).collect(),
        lanes_total: lanes.lanes_total,
        cut: lanes.cut.map(|cut| match cut {
            GraphCut::Lanes => GraphCutDto::Lanes,
            GraphCut::Bytes => GraphCutDto::Bytes,
        }),
    }
}

fn graph_base(base: &GraphBase) -> GraphBaseDto {
    match base {
        GraphBase::Ready { name } => GraphBaseDto::Ready {
            name: visible(name.raw(), NAME_MAX_CHARS),
        },
        GraphBase::NotComputable { reason } => {
            let (reason, name) = match reason {
                BaseGap::BaseUnconfirmed { name } => (GraphBaseGap::BaseUnconfirmed, Some(name)),
                BaseGap::NoBase => (GraphBaseGap::NoBase, None),
                BaseGap::BaseMissing { name } => (GraphBaseGap::BaseMissing, Some(name)),
                BaseGap::RepoUnavailable => (GraphBaseGap::RepoUnavailable, None),
            };
            GraphBaseDto::NotComputable {
                reason,
                name: name.map(|n| visible(n.raw(), NAME_MAX_CHARS)),
            }
        }
    }
}

fn lane(lane: &Lane) -> LaneDto {
    let state = match &lane.state {
        LaneState::Ready {
            merge_base,
            commits,
            total,
        } if is_object_id(merge_base) => LaneStateDto::Ready {
            merge_base: merge_base.clone(),
            total: bounded(*total),
            commits: commits.iter().filter_map(lane_commit).collect(),
        },
        LaneState::Ready { .. } => LaneStateDto::NotComputable {
            reason: LaneGapDto::Unreadable,
        },
        LaneState::NotComputable { reason } => LaneStateDto::NotComputable {
            reason: match reason {
                LaneGap::WorktreeUnavailable => LaneGapDto::WorktreeUnavailable,
                LaneGap::NoCommits => LaneGapDto::NoCommits,
                LaneGap::NoMergeBase => LaneGapDto::NoMergeBase,
                LaneGap::TooSlow => LaneGapDto::TooSlow,
                LaneGap::Unreadable => LaneGapDto::Unreadable,
            },
        },
    };
    LaneDto {
        worktree_key: key(lane.worktree.raw()),
        head: match &lane.head {
            LaneHead::Branch { name } => LaneHeadDto::Branch {
                name: visible(name.raw(), NAME_MAX_CHARS),
            },
            LaneHead::Detached => LaneHeadDto::Detached,
        },
        state,
    }
}

fn lane_commit(commit: &LaneCommit) -> Option<CommitDto> {
    (is_object_id(&commit.oid) && commit.committed_utc_ms >= 0).then(|| CommitDto {
        oid: commit.oid.clone(),
        summary: visible(commit.summary.raw(), TEXT_MAX_CHARS),
        committed_utc_ms: commit.committed_utc_ms,
        merge: commit.merge,
    })
}

/// Whether `event` is one that makes the graph stale: a git change of the repo.
#[must_use]
pub fn graph_trigger(event: &Event) -> bool {
    event.kind == event::GIT_EVENT
}

#[cfg(test)]
mod tests {
    use super::repo_name;

    #[test]
    fn the_repo_name_is_the_folder_without_dot_git() {
        assert_eq!(repo_name("/w/proj/.git"), "proj");
        assert_eq!(repo_name("/w/proj.git"), "proj");
        assert_eq!(repo_name("/w/proj"), "proj");
        assert_eq!(repo_name("C:\\w\\proj\\.git"), "proj");
        assert_eq!(repo_name("/"), "");
    }
}
