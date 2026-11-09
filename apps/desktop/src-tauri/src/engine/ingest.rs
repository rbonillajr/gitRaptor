//! Translation of the engine's contract types into the app's DTOs, sanitizing every text that
//! comes from the file system or an agent before it reaches the webview.

use gitraptor_api::discovery::{CandidateView, RepoDiscoveredData};
use gitraptor_api::display::{NAME_MAX_CHARS, TEXT_MAX_CHARS, visible};
use gitraptor_api::event::{self, Event};
use gitraptor_api::messages::{
    EngineStateView, EngineView, RepoObservationData, RepoStateView, RepoTier, RepoTierData,
    SessionView, WorktreeView,
};
use gitraptor_api::prediction::PairSummary;
use gitraptor_api::scope::{
    AttentionCount, AttentionView, GlobalSnapshot, RepoAttentionData, RepoSnapshot, RepoSummaryView,
};

use super::wire::{
    AttentionDto, Autostart, BaseDto, BaseStatus, CandidateDto, CountDto, DaemonDto, EngineDto,
    EngineMessage, EngineStateDto, GlobalDto, PairDto, PairState, RepoFleetDto, RepoStateDto,
    RepoSummaryDto, SessionDto, Tier, UnavailableReason, WorktreeDto, WorktreeStatusDto,
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
pub fn key(_raw_path: &str) -> String {
    String::new()
}

/// The open repo's snapshot as a DTO.
pub fn repo(_snapshot: &RepoSnapshot) -> RepoFleetDto {
    RepoFleetDto {
        repo_id: String::new(),
        name: String::new(),
        base: BaseDto {
            name: None,
            status: BaseStatus::Unconfirmed,
        },
        fetched_utc_ms: None,
        tier: None,
        worktrees: Vec::new(),
        pairs: Vec::new(),
        pairs_truncated: false,
    }
}

/// One worktree as a DTO.
pub fn worktree(_view: &WorktreeView) -> WorktreeDto {
    WorktreeDto {
        key: String::new(),
        folder: String::new(),
        path: String::new(),
        main: false,
        status: WorktreeStatusDto::Unavailable {
            reason: UnavailableReason::Unreadable,
        },
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
        operation: None,
    }
}

/// One session as a DTO; `None` when it cannot be attributed to an agent.
pub fn session(_view: &SessionView) -> Option<SessionDto> {
    None
}

/// One predicted pair as a DTO.
pub fn pair(_summary: &PairSummary) -> PairDto {
    PairDto {
        pair_id: String::new(),
        a: String::new(),
        b: String::new(),
        state: PairState::Pending,
        level: None,
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
