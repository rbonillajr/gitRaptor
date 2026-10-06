//! The TEA model of the TUI (ADR-CKP-003 § 2): [`Model`], [`Msg`] and
//! [`Cmd`]. The engine part only holds what the engine published, already
//! through the ingest (no raw untrusted text); `view` never reads the clock.

use gitraptor_api::catalog::Layer;
use gitraptor_api::event::Event;
use gitraptor_api::messages::{EngineStateView, ResyncReason};
use gitraptor_api::scope::{AutostartView, Scope, ScopeSnapshot};
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
    pub fn new(lang: Lang, size: Size) -> Self {
        Self {
            engine: EngineReplica::default(),
            ui: Ui {
                lang,
                size,
                notice: None,
                quit: false,
            },
            conn: ConnState::Connecting,
            now_ms: 0,
            dirty: true,
        }
    }
}

/// State of the interface itself.
#[derive(Debug, Clone)]
pub struct Ui {
    pub lang: Lang,
    pub size: Size,
    /// The answer to the last key, so every key changes something visible.
    pub notice: Option<Notice>,
    pub quit: bool,
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
}

/// The selected repo as the view sees it. Its worktrees are US-CKP-001's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoView {
    /// The daemon's id: actions reference ids, never texts.
    pub repo_id: String,
    pub path: SafeText,
    pub worktree_count: usize,
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
}

/// What the channel thread says about the connection.
#[derive(Debug, Clone)]
pub enum ConnEvent {
    State(ConnState),
    /// The handshake ended; who the daemon sees (N5).
    Requester(Option<Requester>),
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
    Quit,
}
