//! The TEA model of the TUI (ADR-CKP-003 § 2): [`Model`], [`Msg`] and
//! [`Cmd`]. The engine part only holds what the engine published, already
//! through the ingest (no raw untrusted text); `view` never reads the clock.

use gitraptor_api::AgentKind;
use gitraptor_api::catalog::Layer;
use gitraptor_api::event::Event;
use gitraptor_api::messages::{
    DivergenceView, EngineStateView, ResyncReason, SessionStateView, SessionsListResult,
    UnavailableReason,
};
use gitraptor_api::scope::{AutostartView, Scope, ScopeSnapshot};
use gitraptor_theme::Theme;
use ratatui::crossterm::event::KeyEvent;

use crate::client::sequence::SeqTrack;
use crate::present::i18n::Lang;

pub use crate::present::SafeText;

/// The whole state of the TUI. Only the main thread owns it.
#[derive(Debug, Clone)]
pub struct Model {
    pub engine: EngineReplica,
    pub ui: Ui,
    pub conn: ConnState,
    /// Wall clock of the view in UTC milliseconds, set by [`Msg::Tick`].
    pub now_ms: i64,
    /// Something visible changed since the last frame.
    pub dirty: bool,
}

impl Model {
    /// A model with the default theme (dark terminal, truecolor, Unicode); the TUI sets the
    /// detected one with [`Model::with_theme`].
    pub fn new(lang: Lang, size: Size) -> Self {
        Self {
            engine: EngineReplica::default(),
            ui: Ui {
                lang,
                size,
                theme: Theme::new(
                    gitraptor_theme::ColorMode::TrueColor,
                    gitraptor_theme::Contrast::Normal,
                    gitraptor_theme::SymbolSet::Unicode,
                ),
                notice: None,
                quit: false,
                pick: Pick::None,
            },
            conn: ConnState::Connecting,
            now_ms: 0,
            dirty: true,
        }
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.ui.theme = theme;
        self
    }
}

/// State of the interface itself.
#[derive(Debug, Clone)]
pub struct Ui {
    pub lang: Lang,
    pub size: Size,
    /// Resolved once at start-up (`--theme`, environment, terminal background).
    pub theme: Theme,
    /// The answer to the last key, so every key changes something visible.
    pub notice: Option<Notice>,
    pub quit: bool,
    /// Choosing the repo when the folder is in none of the observed ones.
    pub pick: Pick,
}

/// The repo to show when the TUI starts outside every observed repo (dogfooding 2026-10-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The folder's repo, or none observed.
    None,
    /// Several observed repos: the developer chooses with ↑↓ and Enter.
    Choosing { selected: usize },
    /// One chosen (or the only one): its snapshot is on its way.
    Opening,
}

/// Terminal size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

/// The visible answer to a key that did nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// The key has no action.
    UnknownKey,
    /// Retry asked while already live.
    AlreadyLive,
    /// Retry asked: reconnecting now.
    Retrying,
}

/// The replica of the engine: the global scope and the selected repo's.
#[derive(Debug, Clone, Default)]
pub struct EngineReplica {
    pub global: ScopeReplica<GlobalView>,
    /// The selected repo, when the current folder is in an observed one.
    pub repo: Option<ScopeReplica<RepoView>>,
    /// Who the daemon sees on this connection (N5); UX only.
    pub requester: Option<Requester>,
    /// The engine publishes the last activity and fetch (`scope.activity`); without it they
    /// are "not available", never "never".
    pub activity: bool,
}

impl EngineReplica {
    /// Every scope is waiting for a snapshot; the data is kept but stale.
    pub fn mark_all_stale(&mut self) {
        self.global.mark_stale();
        if let Some(repo) = &mut self.repo {
            repo.mark_stale();
        }
    }

    /// Whether every scope has a snapshot and follows its stream.
    pub fn all_synced(&self) -> bool {
        self.global.track.is_synced() && self.repo.as_ref().is_none_or(|r| r.track.is_synced())
    }
}

/// One scope's data with its position in the stream (DEP-CKP-6).
#[derive(Debug, Clone)]
pub struct ScopeReplica<T> {
    /// The last data applied; `None` before the first snapshot.
    pub data: Option<T>,
    pub track: SeqTrack,
    /// Kept from before a gap, a resync or a disconnection: never shown
    /// as current.
    pub stale: bool,
    /// Events applied since the last snapshot.
    pub applied: u64,
}

impl<T> Default for ScopeReplica<T> {
    fn default() -> Self {
        Self {
            data: None,
            track: SeqTrack::Waiting,
            stale: false,
            applied: 0,
        }
    }
}

impl<T> ScopeReplica<T> {
    pub fn mark_stale(&mut self) {
        self.track = SeqTrack::Waiting;
        self.stale = self.data.is_some();
    }
}

/// The global scope as the view sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalView {
    pub engine: EngineStateView,
    pub git_version: Option<SafeText>,
    pub autostart: AutostartView,
    pub repo_count: usize,
    /// The observed repos, in the engine's order, to choose one outside all of them.
    pub repos: Vec<RepoChoice>,
    /// ⚡ and ⛔ of each observed repo; `None` while the engine does not count them.
    pub attention: Vec<RepoAttention>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoChoice {
    pub repo_id: String,
    pub name: SafeText,
    pub path: SafeText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoAttention {
    pub repo_id: String,
    pub conflicts: Option<u32>,
    pub denials: Option<u32>,
}

/// The selected repo as the view sees it (US-CKP-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoView {
    /// The daemon's id: actions reference ids, never texts.
    pub repo_id: String,
    pub path: SafeText,
    /// The folder name of the repo, for the header.
    pub name: SafeText,
    /// The base branch ahead/behind is counted against; `None` without one.
    pub base: Option<SafeText>,
    /// As the engine published them, in its order.
    pub worktrees: Vec<WorktreeRow>,
    /// Agent sessions of the repo, from `sessions.list` and then `session.state`.
    pub sessions: Vec<SessionRow>,
    /// When the repo was last fetched (UTC ms); `None` if never (or not published).
    pub fetched_ms: Option<i64>,
    /// Whether this system detects sessions (`sessions.list`); `None` until the list arrives.
    /// Without it the agent is "not available", never "no agent".
    pub detection: Option<bool>,
}

impl RepoView {
    /// Upserts a view of a session: the later `state_since` wins and, at the same instant, an
    /// ended one (ended is never reopened). A view older than the one kept is dropped, so the
    /// list and the stream converge in any order (US-CKP-001, D1).
    pub fn upsert(&mut self, session: SessionRow) {
        match self
            .sessions
            .iter_mut()
            .find(|s| s.session_id == session.session_id)
        {
            Some(kept) => {
                let newer = session.state_since_ms > kept.state_since_ms
                    || (session.state_since_ms == kept.state_since_ms
                        && kept.state != SessionStateView::Ended);
                if newer {
                    *kept = session;
                }
            }
            None => self.sessions.push(session),
        }
    }
}

/// One worktree of the selected repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRow {
    /// Root of the worktree, to show.
    pub path: SafeText,
    /// Folder name of the root: tells apart agents of the same kind.
    pub name: SafeText,
    /// Hash of the raw root: sessions are matched on it, never on sanitized text.
    pub key: u64,
    pub main: bool,
    pub state: WorktreeState,
    /// When the engine last saw it change (UTC ms); `None` while it has not.
    pub last_activity_ms: Option<i64>,
    /// Under the system's temporary folder: a scratch worktree (US-CKP-001).
    pub temporary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeState {
    Ready {
        head: Head,
        /// Changed paths: staged, unstaged and untracked.
        changes: u64,
        divergence: DivergenceView,
    },
    Unavailable(UnavailableReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    Branch(SafeText),
    /// A branch without commits yet.
    Unborn(SafeText),
    /// Directly at a commit, with its short hash when the engine publishes it.
    Detached(Option<SafeText>),
}

/// One agent session, sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub session_id: String,
    /// [`WorktreeRow::key`] of its worktree.
    pub worktree: u64,
    pub kind: AgentKind,
    /// Declared name of an "other agent".
    pub name: Option<SafeText>,
    pub state: SessionStateView,
    /// Orders two views of the same session: the later one wins.
    pub state_since_ms: i64,
}

/// Who the daemon sees on this connection, sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requester {
    Unattributed {
        layer: Layer,
    },
    Agent {
        name: Option<SafeText>,
        layer: Layer,
    },
    Unverified,
}

/// State of the connection (ADR-CKP-003 § 4). Writes only when [`Live`].
///
/// [`Live`]: ConnState::Live
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Syncing,
    Live,
    Resyncing,
    Reconnecting {
        attempt: u32,
    },
    EngineUnavailable,
    Incompatible,
    /// The channel's folder or server failed the peer check (SEC-01, L-06).
    Rejected,
    /// This platform has no channel transport yet.
    Unsupported,
}

impl ConnState {
    pub fn writes_allowed(self) -> bool {
        self == Self::Live
    }
}

/// Everything that reaches `update`.
#[derive(Debug, Clone)]
pub enum Msg {
    Key(KeyEvent),
    Paste(String),
    Resize(Size),
    /// A message of the engine, stamped by the channel thread.
    Engine(Stamped<EngineMsg>),
    Conn(ConnEvent),
    Tick {
        now_ms: i64,
    },
}

/// A message with its stamps on the common monotonic clock (ADR-GRP-011 § 3).
#[derive(Debug, Clone)]
pub struct Stamped<T> {
    /// `t_client_recv`: the frame was read, not decoded yet.
    pub recv_ns: u64,
    /// The frame was decoded.
    pub decoded_ns: u64,
    pub msg: T,
}

/// What the engine says on the stream.
#[derive(Debug, Clone)]
pub enum EngineMsg {
    Snapshot(Box<ScopeSnapshot>),
    Event {
        scope: Scope,
        scope_seq: u64,
        event: Box<Event>,
    },
    Resync {
        scope: Scope,
        reason: ResyncReason,
    },
    /// The sessions of a repo, asked right after its snapshot and subscription.
    Sessions {
        repo_id: String,
        result: Box<SessionsListResult>,
    },
}

/// What the channel thread says about the connection.
#[derive(Debug, Clone)]
pub enum ConnEvent {
    State(ConnState),
    /// The handshake ended; who the daemon sees (N5).
    Requester(Option<Requester>),
    /// Whether the engine publishes the last activity and fetch (`scope.activity`).
    Activity(bool),
    /// The folder is in no observed repo (or there is no folder).
    Unlocated,
}

/// Effects that `update` asks for; they run outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// Take a new snapshot of the scope; `resubscribe` when its
    /// subscription ended.
    Resync {
        scope: Scope,
        resubscribe: bool,
    },
    /// Drop the connection and connect again now.
    Reconnect,
    /// Show this repo (the folder is in none of the observed ones).
    Open {
        repo_id: String,
    },
    Quit,
}
