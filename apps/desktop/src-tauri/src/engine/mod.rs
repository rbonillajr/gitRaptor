//! The app's way to the engine: the seam ([`link`]) and its real implementation through the
//! bundle's `raptor` ([`launcher`]), the connection loop ([`supervisor`]) and what it sends to the
//! webview ([`wire`], [`ingest`], [`seq`]).

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod ingest;
pub mod launcher;
pub mod link;
pub mod seq;
pub mod supervisor;
pub mod wire;

use crate::ipc::CommandError;
use link::Connector;
use supervisor::{HubCmd, Sink, SupervisorHandle};

/// `retry` was asked before any `watch`: there is no supervisor to retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotWatching;

/// Tauri state: one per process, owns the supervisor from the first `watch` until `shutdown`.
pub struct EngineHub {
    handle: Mutex<Option<SupervisorHandle>>,
    connector: Mutex<Option<Box<dyn Connector>>>,
    own_raptor: Option<PathBuf>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl EngineHub {
    pub fn new(connector: Box<dyn Connector>, own_raptor: Option<PathBuf>) -> Self {
        Self {
            handle: Mutex::new(None),
            connector: Mutex::new(Some(connector)),
            own_raptor,
        }
    }

    /// Starts the supervisor on the first call; later calls hand `sink` to the running one.
    pub fn watch(&self, sink: Sink) {
        let mut handle = lock(&self.handle);
        if let Some(running) = handle.as_ref() {
            running.send(HubCmd::Watch(sink));
        } else if let Some(connector) = lock(&self.connector).take() {
            *handle = Some(supervisor::spawn(connector, self.own_raptor.clone(), sink));
        }
    }

    /// Asks the supervisor to connect now, starting the engine when `start`.
    ///
    /// # Errors
    /// [`NotWatching`] when no `watch` has started the supervisor yet.
    pub fn retry(&self, start: bool) -> Result<(), NotWatching> {
        match lock(&self.handle).as_ref() {
            Some(running) if running.send(HubCmd::Retry { start }) => Ok(()),
            _ => Err(NotWatching),
        }
    }

    /// Asks the supervisor to open `repo_id`'s scope.
    ///
    /// # Errors
    /// [`CommandError::NotWatching`] when no `watch` has started the supervisor yet.
    pub fn open(&self, repo_id: String) -> Result<(), CommandError> {
        match lock(&self.handle).as_ref() {
            Some(running) if running.send(HubCmd::Open { repo_id }) => Ok(()),
            _ => Err(CommandError::NotWatching),
        }
    }

    /// Asks the supervisor to stop without waiting for its thread, so the process can exit while a
    /// connection attempt is in flight; a later `retry` reports [`NotWatching`].
    pub fn shutdown(&self) {
        if let Some(running) = lock(&self.handle).take() {
            running.shutdown_detached();
        }
    }
}
