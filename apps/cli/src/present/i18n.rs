//! Typed catalog of the TUI in English and Spanish (ADR-CKP-003 § 10,
//! NFR-10). One enum of messages, one exhaustive `match` per language: a
//! missing translation does not compile. Parameters are [`SafeText`] or
//! numbers.

use gitraptor_api::messages::{EngineStateView, UnavailableReason};

use crate::model::{ConnState, Notice};
use crate::present::SafeText;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

impl Lang {
    /// From `LC_ALL`, `LC_MESSAGES` or `LANG`, in that order; anything that
    /// is not Spanish is English.
    pub fn detect() -> Self {
        let lang = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
        Self::from_locale(lang.as_deref())
    }

    pub fn from_locale(locale: Option<&str>) -> Self {
        match locale {
            Some(l) if l.starts_with("es") => Self::Es,
            _ => Self::En,
        }
    }
}

/// How old the local copy of the remote is: the last fetch of the repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetched {
    /// The engine does not publish it.
    NotAvailable,
    /// The repo was never fetched.
    Never,
    /// Age in milliseconds.
    Ago(i64),
}

/// An age in the largest whole unit: seconds, minutes, hours or days.
fn age(ms: i64) -> (i64, &'static str) {
    let s = ms.max(0) / 1000;
    match s {
        0..60 => (s, "s"),
        60..3_600 => (s / 60, "min"),
        3_600..86_400 => (s / 3_600, "h"),
        _ => (s / 86_400, "d"),
    }
}

/// Every text the TUI paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text<'a> {
    Title,
    Repo(&'a SafeText),
    NoRepo,
    /// The fleet panel, with the reference of ahead/behind (BR-CKP-CALC-001).
    FleetTitle {
        base: Option<&'a SafeText>,
        fetched: Fetched,
    },
    /// The observed repos to choose from (the folder is in none of them).
    PickTitle,
    PickPrompt,
    /// When something last happened, as an age in milliseconds (never negative).
    Ago(i64),
    FleetEmpty,
    FleetWaiting,
    FleetNoRepo,
    ColAgent,
    ColBranch,
    ColFiles,
    ColSync,
    ColActivity,
    /// No agent session in the worktree (BR-CKP-CONS-003): never "human".
    Unattributed,
    /// The engine cannot say whether there are sessions.
    AgentNotAvailable,
    ClaudeCode,
    OtherAgent,
    /// The agent and the folder of its worktree, to tell apart agents of the same kind.
    AgentInWorktree {
        agent: &'a str,
        worktree: &'a str,
    },
    /// More than one present session in the worktree: the first one and how many more.
    AgentAndMore {
        name: &'a str,
        more: usize,
    },
    Detached,
    NoBase,
    BaseMissing,
    NoCommits,
    Unreadable,
    WorktreeUnavailable(UnavailableReason),
    /// A value the engine does not publish yet (BR-CKP-CALC-001).
    NotAvailable,
    RequesterUnknown,
    Engine(Option<EngineStateView>),
    Conn(ConnState),
    Stale,
    ActingAsYourself,
    ActingAsAgent(Option<&'a SafeText>),
    Unverified,
    Notice(Notice),
    KeyQuit,
    KeyRetry,
    KeyUp,
    KeyDown,
    KeyOpen,
    TooSmall {
        width: u16,
        height: u16,
    },
    NeedsTerminal,
}

impl Text<'_> {
    pub fn render(self, lang: Lang) -> String {
        match lang {
            Lang::En => en(self),
            Lang::Es => es(self),
        }
    }
}

fn en(text: Text<'_>) -> String {
    match text {
        Text::Title => "GitRaptor".into(),
        Text::Repo(path) => format!("repo {path}"),
        Text::NoRepo => "no repo selected".into(),
        Text::FleetTitle {
            base: Some(base),
            fetched,
        } => {
            let fetched = match fetched {
                Fetched::NotAvailable => "fetch age not available".into(),
                Fetched::Never => "never fetched".into(),
                Fetched::Ago(ms) => format!("fetched {}", en(Text::Ago(ms))),
            };
            format!("Fleet · ↑↓ vs {base} (local copy, {fetched})")
        }
        Text::FleetTitle { base: None, .. } => "Fleet · no base branch".into(),
        Text::PickTitle => "Observed repos".into(),
        Text::PickPrompt => "This folder is not in an observed repo: choose one.".into(),
        Text::Ago(ms) if ms < 1000 => "just now".into(),
        Text::Ago(ms) => {
            let (n, unit) = age(ms);
            format!("{n} {unit} ago")
        }
        Text::FleetEmpty => "The engine publishes no worktree for this repo.".into(),
        Text::FleetWaiting => "Waiting for the engine…".into(),
        Text::FleetNoRepo => {
            "This folder is not in an observed repo: add it with `raptor repo add`.".into()
        }
        Text::ColAgent => "Agent".into(),
        Text::ColBranch => "Branch".into(),
        Text::ColFiles => "Files".into(),
        Text::ColSync => "↑↓".into(),
        Text::ColActivity => "Activity".into(),
        Text::Unattributed => "Unattributed (you/other)".into(),
        Text::AgentNotAvailable => "agent not available".into(),
        Text::ClaudeCode => "Claude Code".into(),
        Text::OtherAgent => "other agent".into(),
        Text::AgentAndMore { name, more } => format!("{name} +{more}"),
        Text::AgentInWorktree { agent, worktree } => format!("{agent} · {worktree}"),
        Text::Detached => "detached HEAD".into(),
        Text::NoBase => "no base".into(),
        Text::BaseMissing => "base absent".into(),
        Text::NoCommits => "no commits".into(),
        Text::Unreadable => "unreadable".into(),
        Text::WorktreeUnavailable(UnavailableReason::Missing) => "folder missing".into(),
        Text::WorktreeUnavailable(UnavailableReason::Untrusted) => {
            "not trusted by Git (safe.directory)".into()
        }
        Text::WorktreeUnavailable(UnavailableReason::Unreadable) => "unreadable now".into(),
        Text::NotAvailable => "not available".into(),
        Text::RequesterUnknown => "—".into(),
        Text::Engine(None) => "engine —".into(),
        Text::Engine(Some(EngineStateView::WaitingForGit)) => "engine waiting for Git".into(),
        Text::Engine(Some(EngineStateView::NoRepos)) => "engine without repos".into(),
        Text::Engine(Some(EngineStateView::Observing)) => "engine observing".into(),
        Text::Conn(ConnState::Connecting) => "connecting…".into(),
        Text::Conn(ConnState::Syncing) => "syncing…".into(),
        Text::Conn(ConnState::Live) => "live".into(),
        Text::Conn(ConnState::Resyncing) => "resyncing".into(),
        Text::Conn(ConnState::Reconnecting { attempt }) => {
            format!("disconnected, attempt {attempt}")
        }
        Text::Conn(ConnState::EngineUnavailable) => {
            "engine unavailable: run `raptor daemon`".into()
        }
        Text::Conn(ConnState::Incompatible) => "incompatible engine: update raptor".into(),
        Text::Conn(ConnState::Rejected) => {
            "channel rejected: check the permissions of the GitRaptor runtime folder".into()
        }
        Text::Conn(ConnState::Unsupported) => "no engine channel on this platform yet".into(),
        Text::Stale => "out of date".into(),
        Text::ActingAsYourself => "acting as you".into(),
        Text::ActingAsAgent(Some(name)) => format!("acting as {name}"),
        Text::ActingAsAgent(None) => "acting as an agent".into(),
        Text::Unverified => "identity not verified: read only".into(),
        Text::Notice(Notice::UnknownKey) => "key without an action".into(),
        Text::Notice(Notice::AlreadyLive) => "already live".into(),
        Text::Notice(Notice::Retrying) => "reconnecting now…".into(),
        Text::KeyQuit => "quit".into(),
        Text::KeyRetry => "retry".into(),
        Text::KeyUp => "up".into(),
        Text::KeyDown => "down".into(),
        Text::KeyOpen => "open".into(),
        Text::TooSmall { width, height } => {
            format!("Terminal too small ({width}×{height}): at least 80×24 is needed.")
        }
        Text::NeedsTerminal => {
            "the cockpit needs a terminal; for plain output use `raptor status`".into()
        }
    }
}

fn es(text: Text<'_>) -> String {
    match text {
        Text::Title => "GitRaptor".into(),
        Text::Repo(path) => format!("repo {path}"),
        Text::NoRepo => "sin repo seleccionado".into(),
        Text::FleetTitle {
            base: Some(base),
            fetched,
        } => {
            let fetched = match fetched {
                Fetched::NotAvailable => "antigüedad no disponible".into(),
                Fetched::Never => "sin fetch".into(),
                Fetched::Ago(ms) => format!("fetch {}", es(Text::Ago(ms))),
            };
            format!("Flota · ↑↓ respecto a {base} (copia local, {fetched})")
        }
        Text::FleetTitle { base: None, .. } => "Flota · sin rama base".into(),
        Text::PickTitle => "Repos observados".into(),
        Text::PickPrompt => "Esta carpeta no está en un repo observado: elige uno.".into(),
        Text::Ago(ms) if ms < 1000 => "ahora".into(),
        Text::Ago(ms) => {
            let (n, unit) = age(ms);
            format!("hace {n} {unit}")
        }
        Text::FleetEmpty => "El motor no publica ningún worktree de este repo.".into(),
        Text::FleetWaiting => "Esperando al motor…".into(),
        Text::FleetNoRepo => {
            "Esta carpeta no está en un repo observado: añádelo con `raptor repo add`.".into()
        }
        Text::ColAgent => "Agente".into(),
        Text::ColBranch => "Rama".into(),
        Text::ColFiles => "Arch.".into(),
        Text::ColSync => "↑↓".into(),
        Text::ColActivity => "Actividad".into(),
        Text::Unattributed => "Tú u otro (sin atribuir)".into(),
        Text::AgentNotAvailable => "agente no disponible".into(),
        Text::ClaudeCode => "Claude Code".into(),
        Text::OtherAgent => "otro agente".into(),
        Text::AgentAndMore { name, more } => format!("{name} +{more}"),
        Text::AgentInWorktree { agent, worktree } => format!("{agent} · {worktree}"),
        Text::Detached => "HEAD separado".into(),
        Text::NoBase => "sin base".into(),
        Text::BaseMissing => "falta base".into(),
        Text::NoCommits => "sin commits".into(),
        Text::Unreadable => "ilegible".into(),
        Text::WorktreeUnavailable(UnavailableReason::Missing) => "falta la carpeta".into(),
        Text::WorktreeUnavailable(UnavailableReason::Untrusted) => {
            "Git no confía en él (safe.directory)".into()
        }
        Text::WorktreeUnavailable(UnavailableReason::Unreadable) => "ilegible ahora".into(),
        Text::NotAvailable => "no disponible".into(),
        Text::RequesterUnknown => "—".into(),
        Text::Engine(None) => "motor —".into(),
        Text::Engine(Some(EngineStateView::WaitingForGit)) => "motor esperando Git".into(),
        Text::Engine(Some(EngineStateView::NoRepos)) => "motor sin repos".into(),
        Text::Engine(Some(EngineStateView::Observing)) => "motor observando".into(),
        Text::Conn(ConnState::Connecting) => "conectando…".into(),
        Text::Conn(ConnState::Syncing) => "sincronizando…".into(),
        Text::Conn(ConnState::Live) => "en vivo".into(),
        Text::Conn(ConnState::Resyncing) => "resincronizando".into(),
        Text::Conn(ConnState::Reconnecting { attempt }) => {
            format!("desconectado, intento {attempt}")
        }
        Text::Conn(ConnState::EngineUnavailable) => {
            "motor no disponible: ejecuta `raptor daemon`".into()
        }
        Text::Conn(ConnState::Incompatible) => "motor incompatible: actualiza raptor".into(),
        Text::Conn(ConnState::Rejected) => {
            "canal rechazado: revisa los permisos de la carpeta de ejecución de GitRaptor".into()
        }
        Text::Conn(ConnState::Unsupported) => {
            "todavía no hay canal del motor en esta plataforma".into()
        }
        Text::Stale => "desactualizado".into(),
        Text::ActingAsYourself => "actúas como tú".into(),
        Text::ActingAsAgent(Some(name)) => format!("actúas como {name}"),
        Text::ActingAsAgent(None) => "actúas como un agente".into(),
        Text::Unverified => "identidad no verificada: solo lectura".into(),
        Text::Notice(Notice::UnknownKey) => "tecla sin acción".into(),
        Text::Notice(Notice::AlreadyLive) => "ya estás en vivo".into(),
        Text::Notice(Notice::Retrying) => "reconectando ahora…".into(),
        Text::KeyQuit => "salir".into(),
        Text::KeyRetry => "reintentar".into(),
        Text::KeyUp => "subir".into(),
        Text::KeyDown => "bajar".into(),
        Text::KeyOpen => "abrir".into(),
        Text::TooSmall { width, height } => {
            format!("Terminal demasiado pequeña ({width}×{height}): hacen falta al menos 80×24.")
        }
        Text::NeedsTerminal => {
            "el cockpit necesita una terminal; para salida de texto usa `raptor status`".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_only_for_spanish_locales() {
        assert_eq!(Lang::from_locale(Some("es_ES.UTF-8")), Lang::Es);
        assert_eq!(Lang::from_locale(Some("en_US.UTF-8")), Lang::En);
        assert_eq!(Lang::from_locale(Some("C")), Lang::En);
        assert_eq!(Lang::from_locale(None), Lang::En);
    }

    #[test]
    fn every_connection_state_has_both_languages() {
        let states = [
            ConnState::Connecting,
            ConnState::Syncing,
            ConnState::Live,
            ConnState::Resyncing,
            ConnState::Reconnecting { attempt: 3 },
            ConnState::EngineUnavailable,
            ConnState::Incompatible,
            ConnState::Rejected,
            ConnState::Unsupported,
        ];
        for state in states {
            let en = Text::Conn(state).render(Lang::En);
            let es = Text::Conn(state).render(Lang::Es);
            assert!(!en.is_empty() && !es.is_empty());
            assert_ne!(en, es, "{state:?}");
        }
    }
}
