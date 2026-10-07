//! Typed catalog of the TUI in English and Spanish (ADR-CKP-003 § 10,
//! NFR-10). One enum of messages, one exhaustive `match` per language: a
//! missing translation does not compile. Parameters are [`SafeText`] or
//! numbers.

use std::sync::OnceLock;

use gitraptor_api::messages::{EngineStateView, ResyncReason, UnavailableReason};
use gitraptor_api::rpc::{ErrorCode, InvalidReason, ScopeRefusal};

use crate::model::{ConnState, Notice};
use crate::present::SafeText;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

/// The language chosen with `--lang` for the whole process; read by [`Lang::detect`].
static CHOSEN: OnceLock<Lang> = OnceLock::new();

/// The variable that chooses the language without the command line.
pub const LANG_VAR: &str = "GITRAPTOR_LANG";

impl Lang {
    /// `--lang`, then `GITRAPTOR_LANG`, then `LC_ALL`, `LC_MESSAGES` or `LANG`, and `en`
    /// (ADR-CKP-003 § 10).
    pub fn detect() -> Self {
        if let Some(lang) = CHOSEN.get() {
            return *lang;
        }
        Self::pick(None, |k| std::env::var(k).ok())
    }

    /// Fixes the language of the process from `--lang` (the binary calls it once, before
    /// anything is printed). Without a flag, the environment decides.
    pub fn choose(flag: Option<Self>) -> Self {
        let lang = Self::pick(flag, |k| std::env::var(k).ok());
        let _ = CHOSEN.set(lang);
        lang
    }

    /// The order of precedence, over any environment. An unknown `GITRAPTOR_LANG` is
    /// ignored and the locale decides: it never fails.
    pub fn pick(flag: Option<Self>, env: impl Fn(&str) -> Option<String>) -> Self {
        if let Some(lang) = flag {
            return lang;
        }
        if let Some(lang) = env(LANG_VAR).as_deref().and_then(Self::parse) {
            return lang;
        }
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| env(k).filter(|v| !v.is_empty()));
        Self::from_locale(locale.as_deref())
    }

    /// `en` or `es` (any case, also as a locale such as `es_ES.UTF-8`); anything else is
    /// `None`.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        let tag = value.split(['_', '-', '.']).next().unwrap_or_default();
        match tag {
            "en" => Some(Self::En),
            "es" => Some(Self::Es),
            _ => None,
        }
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
    /// No agent session in the worktree (BR-CKP-CONS-003): never "human", and the folder of
    /// the worktree, to tell the rows apart.
    NoAgent {
        worktree: &'a str,
    },
    /// What the ○ of a row without an agent means, for the key hints line.
    NoAgentLegend,
    /// A worktree under the system's temporary folder.
    Temporary,
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
    /// The last commit of a worktree with its declared authorship (US-CKP-026): the person,
    /// the agents of its trailers, the agent that ran it without one, or an unconfirmed hint.
    /// A person's commit without an agent is "commit by Ana", nothing more: not a fault.
    LastCommit {
        merge: bool,
        author: &'a str,
        /// The agents of the trailers, already named and joined; empty without any.
        agents: &'a str,
        ran_by: Option<&'a str>,
        inferred: Option<&'a str>,
    },
    /// A detached `HEAD`: the short hash it is at, when published.
    NoBranch(Option<&'a str>),
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
    KeySuspend,
    /// An error of the contract's frozen list, by its code (N7), never by its `message`.
    EngineError(ErrorCode),
    /// An error a module declared after the list froze, by its stable name (`error.<name>`).
    ModuleError(&'a str),
    /// A code the contract does not have (a newer engine).
    UnknownError(i64),
    /// Why the caller's scope was refused (`data` of `scope-refused`).
    ScopeRefused(ScopeRefusal),
    /// Why a path or a name was refused (`data` of `invalid-params`).
    Invalid(InvalidReason),
    /// Why the engine asked for a new snapshot (`events.resync`).
    Resync(ResyncReason),
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
        Text::NoAgent { worktree } => format!("No agent · {worktree}"),
        Text::NoAgentLegend => "no agent: changes by you or another tool".into(),
        Text::Temporary => "temporary".into(),
        Text::AgentNotAvailable => "agent not available".into(),
        Text::ClaudeCode => "Claude Code".into(),
        Text::OtherAgent => "other agent".into(),
        Text::LastCommit {
            merge,
            author,
            agents,
            ran_by,
            inferred,
        } => {
            let mut out = format!("{} by {author}", if merge { "merge" } else { "commit" });
            if !agents.is_empty() {
                out.push_str(&format!(" with {agents}"));
            }
            if let Some(agent) = ran_by {
                out.push_str(&format!(" · run by {agent}"));
                if agents.is_empty() {
                    out.push_str(" · no trailer");
                }
            } else if let Some(agent) = inferred {
                out.push_str(&format!(" · possibly {agent} (inferred)"));
            }
            out
        }
        Text::AgentAndMore { name, more } => format!("{name} +{more}"),
        Text::AgentInWorktree { agent, worktree } => format!("{agent} · {worktree}"),
        Text::NoBranch(Some(commit)) => format!("{commit} (no branch)"),
        Text::NoBranch(None) => "(no branch)".into(),
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
        Text::Conn(ConnState::Starting) => "starting the engine…".into(),
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
        Text::Notice(Notice::Starting) => "the engine is starting; wait for it".into(),
        Text::Notice(Notice::SuspendUnsupported) => {
            "suspending is not available on this platform".into()
        }
        Text::KeyQuit => "quit".into(),
        Text::KeyRetry => "retry".into(),
        Text::KeyUp => "up".into(),
        Text::KeyDown => "down".into(),
        Text::KeyOpen => "open".into(),
        Text::KeySuspend => "suspend".into(),
        Text::EngineError(code) => en_error(code).into(),
        Text::ModuleError(name) => module_error(name, Lang::En)
            .map_or_else(|| format!("the engine refused it ({name})"), Into::into),
        Text::UnknownError(code) => format!("the engine answered an unknown error ({code})"),
        Text::ScopeRefused(reason) => match reason {
            ScopeRefusal::NoWorkingFolder => "no readable working folder",
            ScopeRefusal::NotObserved => "the folder is not in an observed repo",
            ScopeRefusal::NotAllowlisted => "the repo is not in the MCP allowlist",
            ScopeRefusal::UnattributedOverMcp => "an unattributed caller cannot use MCP",
            ScopeRefusal::ForeignWorktree => "the worktree belongs to another repo",
        }
        .into(),
        Text::Invalid(reason) => match reason {
            InvalidReason::Empty => "empty value",
            InvalidReason::TooLong => "value too long",
            InvalidReason::NotAbsolute => "the path is not absolute",
            InvalidReason::ControlCharacter => "the value has control characters",
            InvalidReason::UncOrDevice => "UNC or device paths are not accepted",
            InvalidReason::DeviceName => "reserved device name",
            InvalidReason::AlternateStream => "alternate data streams are not accepted",
            InvalidReason::OutsideObserved => "outside the observed repos",
            InvalidReason::InvalidRef => "not a valid Git reference",
            InvalidReason::ReservedName => "reserved name",
        }
        .into(),
        Text::Resync(reason) => match reason {
            ResyncReason::SlowConsumer => "the view fell behind the engine",
            ResyncReason::ReplayUnavailable => "the engine no longer has the missed events",
            ResyncReason::DaemonRestarted => "the engine restarted",
            ResyncReason::ScopeClosed => "the repo stopped being observed",
        }
        .into(),
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
        Text::NoAgent { worktree } => format!("Sin agente · {worktree}"),
        Text::NoAgentLegend => "sin agente: cambios tuyos o de otra herramienta".into(),
        Text::Temporary => "temporal".into(),
        Text::AgentNotAvailable => "agente no disponible".into(),
        Text::ClaudeCode => "Claude Code".into(),
        Text::OtherAgent => "otro agente".into(),
        Text::LastCommit {
            merge,
            author,
            agents,
            ran_by,
            inferred,
        } => {
            let mut out = format!("{} de {author}", if merge { "merge" } else { "commit" });
            if !agents.is_empty() {
                out.push_str(&format!(" con {agents}"));
            }
            if let Some(agent) = ran_by {
                out.push_str(&format!(" · ejecutado por {agent}"));
                if agents.is_empty() {
                    out.push_str(" · sin trailer");
                }
            } else if let Some(agent) = inferred {
                out.push_str(&format!(" · posible {agent} (inferido)"));
            }
            out
        }
        Text::AgentAndMore { name, more } => format!("{name} +{more}"),
        Text::AgentInWorktree { agent, worktree } => format!("{agent} · {worktree}"),
        Text::NoBranch(Some(commit)) => format!("{commit} (sin rama)"),
        Text::NoBranch(None) => "(sin rama)".into(),
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
        Text::Conn(ConnState::Starting) => "arrancando el motor…".into(),
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
        Text::Notice(Notice::Starting) => "el motor está arrancando; espera".into(),
        Text::Notice(Notice::SuspendUnsupported) => {
            "suspender no está disponible en esta plataforma".into()
        }
        Text::KeyQuit => "salir".into(),
        Text::KeyRetry => "reintentar".into(),
        Text::KeyUp => "subir".into(),
        Text::KeyDown => "bajar".into(),
        Text::KeyOpen => "abrir".into(),
        Text::KeySuspend => "suspender".into(),
        Text::EngineError(code) => es_error(code).into(),
        Text::ModuleError(name) => module_error(name, Lang::Es)
            .map_or_else(|| format!("el motor lo rechazó ({name})"), Into::into),
        Text::UnknownError(code) => format!("el motor respondió un error desconocido ({code})"),
        Text::ScopeRefused(reason) => match reason {
            ScopeRefusal::NoWorkingFolder => "no hay una carpeta de trabajo legible",
            ScopeRefusal::NotObserved => "la carpeta no está en un repo observado",
            ScopeRefusal::NotAllowlisted => "el repo no está en la lista permitida del MCP",
            ScopeRefusal::UnattributedOverMcp => "quien no está atribuido no puede usar el MCP",
            ScopeRefusal::ForeignWorktree => "el worktree es de otro repo",
        }
        .into(),
        Text::Invalid(reason) => match reason {
            InvalidReason::Empty => "valor vacío",
            InvalidReason::TooLong => "valor demasiado largo",
            InvalidReason::NotAbsolute => "la ruta no es absoluta",
            InvalidReason::ControlCharacter => "el valor tiene caracteres de control",
            InvalidReason::UncOrDevice => "no se aceptan rutas UNC ni de dispositivo",
            InvalidReason::DeviceName => "nombre de dispositivo reservado",
            InvalidReason::AlternateStream => "no se aceptan flujos de datos alternativos",
            InvalidReason::OutsideObserved => "fuera de los repos observados",
            InvalidReason::InvalidRef => "no es una referencia de Git válida",
            InvalidReason::ReservedName => "nombre reservado",
        }
        .into(),
        Text::Resync(reason) => match reason {
            ResyncReason::SlowConsumer => "la vista se quedó atrás del motor",
            ResyncReason::ReplayUnavailable => "el motor ya no tiene los eventos perdidos",
            ResyncReason::DaemonRestarted => "el motor se reinició",
            ResyncReason::ScopeClosed => "el repo dejó de observarse",
        }
        .into(),
        Text::TooSmall { width, height } => {
            format!("Terminal demasiado pequeña ({width}×{height}): hacen falta al menos 80×24.")
        }
        Text::NeedsTerminal => {
            "el cockpit necesita una terminal; para salida de texto usa `raptor status`".into()
        }
    }
}

impl Text<'static> {
    /// The text of any error code of the contract (N7): the frozen list, a module's own,
    /// or unknown.
    pub fn error(code: i64) -> Self {
        match ErrorCode::from_code(code) {
            Some(code) => Self::EngineError(code),
            None => gitraptor_api::rpc::error_name(code)
                .map_or(Self::UnknownError(code), Self::ModuleError),
        }
    }
}

/// The codes modules declared after the list froze (`rpc::module_errors`), by name. A
/// module that adds one adds its line here; the catalog test fails until it does.
fn module_error(name: &str, lang: Lang) -> Option<&'static str> {
    MODULE_ERRORS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, en, es)| match lang {
            Lang::En => *en,
            Lang::Es => *es,
        })
}

/// `(name, en, es)` of each module's own code. None has declared one yet.
const MODULE_ERRORS: &[(&str, &str, &str)] = &[];

fn en_error(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::ParseError => "the engine could not read the request",
        ErrorCode::InvalidRequest => "the request is not valid",
        ErrorCode::MethodNotFound => "the engine does not know that request",
        ErrorCode::InvalidParams => "the request has invalid parameters",
        ErrorCode::Internal => "internal error of the engine",
        ErrorCode::HandshakeRequired => "the channel needs a handshake first",
        ErrorCode::IncompatibleProtocol => "the engine speaks another protocol version",
        ErrorCode::ReservedRefused => "only a person at a terminal can do that",
        ErrorCode::NotImplemented => "the engine does not do that yet",
        ErrorCode::RateLimited => "too many requests; wait a moment",
        ErrorCode::LimitReached => "connection or subscription limit reached",
        ErrorCode::ResyncRequired => "the view must be refreshed from a new snapshot",
        ErrorCode::PriorSnapshotFailed => "the prior snapshot failed: nothing was changed",
        ErrorCode::NotFound => "not found",
        ErrorCode::ScopeRefused => "that scope is refused",
        ErrorCode::OperationFailed => "the operation failed; its prior snapshot can undo it",
        ErrorCode::IdentityUnverified => "your identity changed: reconnect",
        ErrorCode::RepoRejected => "the repo was rejected",
        ErrorCode::OperationRejected => "the operation was refused before running",
        ErrorCode::RegistrationRejected => "the registration was refused",
        ErrorCode::GuardRejected => "the Guardrails install was refused",
    }
}

fn es_error(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::ParseError => "el motor no pudo leer la petición",
        ErrorCode::InvalidRequest => "la petición no es válida",
        ErrorCode::MethodNotFound => "el motor no conoce esa petición",
        ErrorCode::InvalidParams => "la petición tiene parámetros no válidos",
        ErrorCode::Internal => "error interno del motor",
        ErrorCode::HandshakeRequired => "el canal necesita antes un saludo",
        ErrorCode::IncompatibleProtocol => "el motor habla otra versión del protocolo",
        ErrorCode::ReservedRefused => "solo una persona en una terminal puede hacer eso",
        ErrorCode::NotImplemented => "el motor todavía no hace eso",
        ErrorCode::RateLimited => "demasiadas peticiones; espera un momento",
        ErrorCode::LimitReached => "se alcanzó el límite de conexiones o suscripciones",
        ErrorCode::ResyncRequired => "la vista debe rehacerse desde una instantánea nueva",
        ErrorCode::PriorSnapshotFailed => "falló la instantánea previa: no se cambió nada",
        ErrorCode::NotFound => "no encontrado",
        ErrorCode::ScopeRefused => "ese ámbito está rechazado",
        ErrorCode::OperationFailed => "la operación falló; su instantánea previa puede deshacerla",
        ErrorCode::IdentityUnverified => "tu identidad cambió: reconecta",
        ErrorCode::RepoRejected => "el repo fue rechazado",
        ErrorCode::OperationRejected => "la operación se rechazó antes de ejecutarse",
        ErrorCode::RegistrationRejected => "el registro fue rechazado",
        ErrorCode::GuardRejected => "se rechazó la instalación de Guardrails",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            vars.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn lang_precedence() {
        let all = [
            ("GITRAPTOR_LANG", "es"),
            ("LC_ALL", "en_US.UTF-8"),
            ("LANG", "en_US.UTF-8"),
        ];
        // `--lang` beats everything.
        assert_eq!(Lang::pick(Some(Lang::En), env(&all)), Lang::En);
        // `GITRAPTOR_LANG` beats the locale.
        assert_eq!(Lang::pick(None, env(&all)), Lang::Es);
        // An unknown value is ignored: the locale decides, nothing fails.
        assert_eq!(
            Lang::pick(None, env(&[("GITRAPTOR_LANG", "fr"), ("LANG", "es_ES")])),
            Lang::Es
        );
        assert_eq!(
            Lang::pick(None, env(&[("GITRAPTOR_LANG", ""), ("LANG", "en_GB")])),
            Lang::En
        );
        // Then LC_ALL, LC_MESSAGES and LANG, in that order; then English.
        assert_eq!(
            Lang::pick(None, env(&[("LC_MESSAGES", "es_MX"), ("LANG", "en_US")])),
            Lang::Es
        );
        assert_eq!(
            Lang::pick(None, env(&[("LC_ALL", "C"), ("LC_MESSAGES", "es_MX")])),
            Lang::En
        );
        assert_eq!(Lang::pick(None, env(&[])), Lang::En);
        assert_eq!(Lang::parse("ES"), Some(Lang::Es));
        assert_eq!(Lang::parse("en-GB"), Some(Lang::En));
        assert_eq!(Lang::parse("fr"), None);
    }

    /// V8: every code and typed reason of the contract (N7) has a message in en and es,
    /// different, and no module's code falls back to the generic text.
    #[test]
    fn every_contract_code_has_both_languages() {
        let mut texts: Vec<Text<'static>> = Vec::new();
        texts.extend(ErrorCode::ALL.map(Text::EngineError));
        texts.extend(ScopeRefusal::ALL.map(Text::ScopeRefused));
        texts.extend(InvalidReason::ALL.map(Text::Invalid));
        texts.extend(ResyncReason::ALL.map(Text::Resync));
        for text in &texts {
            let en = text.render(Lang::En);
            let es = text.render(Lang::Es);
            assert!(!en.is_empty() && !es.is_empty(), "{text:?}");
            assert_ne!(en, es, "{text:?}");
        }
        for code in ErrorCode::ALL {
            assert_eq!(Text::error(code.code()), Text::EngineError(code));
        }
        for spec in gitraptor_api::rpc::module_errors() {
            assert_eq!(Text::error(spec.code), Text::ModuleError(spec.name));
            for lang in [Lang::En, Lang::Es] {
                assert!(
                    module_error(spec.name, lang).is_some(),
                    "error.{} has no text in {lang:?}",
                    spec.name
                );
            }
        }
        assert_eq!(Text::error(-39_999), Text::UnknownError(-39_999));
        assert!(Text::error(-39_999).render(Lang::Es).contains("-39999"));
    }

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
            ConnState::Starting,
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
