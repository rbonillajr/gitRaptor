//! The channel client library plugged into the TUI's [`Connector`] and
//! [`Link`]. The only module of the library that imports `gitraptor_core`:
//! the client still lives there (TS-GRP-004) and moves to `crates/api`
//! later (pending, owner: INF-CKP-001). It brings the peer check of the
//! channel before the handshake (L-06) and the on-demand start (§ 5).

use std::time::Duration;

use gitraptor_api::messages::ClientKind;
use gitraptor_api::scope::ConnectionRequester;
use gitraptor_core::client::{
    Client, ClientError, ClientOptions, Incoming as CoreIncoming, ensure_daemon,
};
use gitraptor_core::profile::ProfileDirs;
use serde_json::Value;

use crate::client::{Connector, Incoming, Link, LinkError};

/// Connects through the client library of the engine.
pub struct EngineConnector {
    options: ClientOptions,
}

impl EngineConnector {
    pub fn new(options: ClientOptions) -> Self {
        Self { options }
    }

    /// The user's profile, as `raptor` resolves it.
    pub fn for_current_user() -> Result<Self, String> {
        let dirs = ProfileDirs::resolve().map_err(|err| err.to_string())?;
        Ok(Self::new(ClientOptions::new(dirs, ClientKind::Cli)))
    }
}

impl Connector for EngineConnector {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError> {
        ensure_daemon(&self.options)
            .map(|client| Box::new(EngineLink(client)) as Box<dyn Link>)
            .map_err(link_error)
    }
}

struct EngineLink(Client);

impl Link for EngineLink {
    fn requester(&self) -> Option<ConnectionRequester> {
        self.0.hello().requester.clone()
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        self.0.call(method, params).map_err(link_error)
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<Incoming>, LinkError> {
        Ok(match self.0.next_incoming(timeout).map_err(link_error)? {
            None => None,
            Some(CoreIncoming::Buffered {
                recv_ns,
                notification,
            }) => Some(Incoming::Decoded {
                recv_ns,
                notification,
            }),
            Some(CoreIncoming::Frame { recv_ns, bytes }) => {
                Some(Incoming::Frame { recv_ns, bytes })
            }
        })
    }
}

fn link_error(err: ClientError) -> LinkError {
    match err {
        ClientError::NotRunning | ClientError::StartTimeout => LinkError::EngineUnavailable,
        ClientError::ChannelRejected | ClientError::NotAuthentic => LinkError::Rejected,
        ClientError::Incompatible(_) | ClientError::ClientTooOld(_) => LinkError::Incompatible,
        ClientError::TransportUnsupported | ClientError::Unsupported(_) => LinkError::Unsupported,
        ClientError::Rpc(_) => LinkError::Refused,
        ClientError::Io(_) | ClientError::Protocol(_) => LinkError::Lost,
    }
}
