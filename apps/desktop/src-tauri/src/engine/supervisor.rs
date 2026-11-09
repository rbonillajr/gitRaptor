//! The connection loop between the app and the engine, on its own thread.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::link::Connector;
use super::wire::EngineMessage;

/// Where the supervisor delivers its messages.
pub enum Sink {
    Channel(tauri::ipc::Channel<EngineMessage>),
    Collect(Sender<EngineMessage>),
}

impl Sink {
    /// Delivers `msg`; a listener that is gone is ignored (the next `watch` replaces it).
    pub fn send(&self, msg: EngineMessage) {
        match self {
            Self::Channel(channel) => drop(channel.send(msg)),
            Self::Collect(tx) => drop(tx.send(msg)),
        }
    }
}

/// An order for the supervisor thread.
pub enum HubCmd {
    Watch(Sink),
    Retry { start: bool },
    Open { repo_id: String },
    Shutdown,
}

/// The running supervisor: dropping it shuts the thread down and waits for it.
pub struct SupervisorHandle {
    tx: Sender<HubCmd>,
    thread: Option<JoinHandle<()>>,
}

impl SupervisorHandle {
    /// Sends an order; `false` when the thread is gone.
    pub fn send(&self, cmd: HubCmd) -> bool {
        self.tx.send(cmd).is_ok()
    }

    /// Shuts the thread down and waits for it.
    pub fn join(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        drop(self.tx.send(HubCmd::Shutdown));
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

impl Drop for SupervisorHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the supervisor thread.
pub fn spawn(
    connector: Box<dyn Connector>,
    own_raptor: Option<PathBuf>,
    sink: Sink,
) -> SupervisorHandle {
    let (tx, rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("gitraptor-engine".to_owned())
        .spawn(move || run(connector, own_raptor, sink, rx))
        .ok();
    SupervisorHandle { tx, thread }
}

/// The wait before reconnection attempt `attempt` (1-based): 250 ms doubling, capped at 5 s.
pub fn backoff(attempt: u32) -> Duration {
    let exp = attempt.saturating_sub(1).min(5);
    Duration::from_millis(250 * (1u64 << exp)).min(Duration::from_secs(5))
}

// STUB: the connection loop; the only `todo!()` on a production path.
#[allow(clippy::needless_pass_by_value)]
fn run(
    _connector: Box<dyn Connector>,
    _own_raptor: Option<PathBuf>,
    _sink: Sink,
    _rx: Receiver<HubCmd>,
) {
    todo!("supervisor loop")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_from_250_ms_and_caps_at_5_s() {
        let ms: Vec<u128> = (1..=8).map(|n| backoff(n).as_millis()).collect();
        assert_eq!(ms, [250, 500, 1000, 2000, 4000, 5000, 5000, 5000]);
        assert_eq!(backoff(0).as_millis(), 250);
        assert_eq!(backoff(u32::MAX).as_millis(), 5000);
    }
}
