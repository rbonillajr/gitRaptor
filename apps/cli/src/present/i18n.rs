//! Typed catalog of the TUI in English and Spanish (ADR-CKP-003 § 10,
//! NFR-10). One enum of messages, one exhaustive `match` per language: a
//! missing translation does not compile. Parameters are [`SafeText`] or
//! numbers.

use gitraptor_api::messages::EngineStateView;

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

/// Every text the TUI paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text<'a> {
    Title,
    Repo(&'a SafeText),
    NoRepo,
    FleetTitle,
    FleetEmpty,
    Engine(Option<EngineStateView>),
    Conn(ConnState),
    Stale,
    ActingAsYourself,
    ActingAsAgent(Option<&'a SafeText>),
    Unverified,
    Notice(Notice),
    KeyQuit,
    KeyRetry,
    TooSmall { width: u16, height: u16 },
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
        Text::FleetTitle => "Fleet".into(),
        Text::FleetEmpty => "The live fleet will appear here.".into(),
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
        Text::FleetTitle => "Flota".into(),
        Text::FleetEmpty => "Aquí aparecerá la flota en vivo.".into(),
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
