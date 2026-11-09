//! Translation of the engine's contract types into the app's DTOs, sanitizing every text that
//! comes from the file system or an agent.
// STUB: every function returns a placeholder until the supervisor lands.

use gitraptor_api::discovery::CandidateView;
use gitraptor_api::messages::EngineView;
use gitraptor_api::rpc::Notification;
use gitraptor_api::scope::{GlobalSnapshot, RepoSummaryView};

use super::wire::{
    AttentionDto, Autostart, CandidateDto, CountDto, DaemonDto, EngineDto, EngineMessage,
    EngineStateDto, GlobalDto, RepoStateDto, RepoSummaryDto,
};

fn placeholder_engine() -> EngineDto {
    EngineDto {
        state: EngineStateDto::WaitingForGit,
        git_version: None,
    }
}

/// The global snapshot as a DTO.
// STUB
pub fn global(_snapshot: &GlobalSnapshot) -> GlobalDto {
    GlobalDto {
        engine: placeholder_engine(),
        daemon: DaemonDto {
            version: String::new(),
            protocol: 0,
        },
        autostart: Autostart::Unknown,
        repos: Vec::new(),
    }
}

/// One repo of the global scope as a DTO.
// STUB
pub fn repo_summary(_repo: &RepoSummaryView) -> RepoSummaryDto {
    RepoSummaryDto {
        repo_id: String::new(),
        name: String::new(),
        path: String::new(),
        state: RepoStateDto::Unavailable,
        tier: None,
        attention: AttentionDto {
            conflicts: CountDto::Unavailable,
            denials: CountDto::Unavailable,
        },
    }
}

/// A discovered repo as a DTO.
// STUB
pub fn candidate(_candidate: &CandidateView) -> CandidateDto {
    CandidateDto {
        name: String::new(),
        path: String::new(),
    }
}

/// The engine part of a snapshot or an `engine.state` event as a DTO.
// STUB
pub fn engine(_engine: &EngineView) -> EngineDto {
    placeholder_engine()
}

/// The messages a global-scope event produces; empty for events the app ignores.
// STUB
pub fn global_event(_event: &Notification) -> Vec<EngineMessage> {
    Vec::new()
}
