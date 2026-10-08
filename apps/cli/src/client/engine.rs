//! The channel client library of `crates/api` plugged into the TUI's [`Connector`] and
//! [`Link`] (INF-CKP-001 Entrega 2b). It brings the peer check of the channel before the
//! handshake (L-06). How a daemon is started is injected as a [`Launch`]: the binary builds
//! it from the engine (`gitraptor_core::client::ClientOptions::launcher`), so nothing of the
//! TUI imports the engine nor launches a process (ADR-CKP-003, Enmienda 2026-10-08).

use std::time::Duration;

use gitraptor_api::client::{
    Client, ClientError, Connect, Incoming as ApiIncoming, Launch, ensure_daemon_with,
};
use gitraptor_api::scope::ConnectionRequester;
use serde_json::Value;

use crate::client::{Connector, Incoming, Link, LinkError, Refusal};

/// Connects through the client library of `crates/api`.
pub struct EngineConnector {
    connect: Connect,
    launch: Box<dyn Launch>,
}

impl EngineConnector {
    /// `connect` says where the channel is; `launch` starts a daemon that is not running.
    pub fn new(connect: Connect, launch: Box<dyn Launch>) -> Self {
        Self { connect, launch }
    }
}

impl Connector for EngineConnector {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError> {
        self.connect_starting(&mut || {})
    }

    fn connect_starting(&mut self, starting: &mut dyn FnMut()) -> Result<Box<dyn Link>, LinkError> {
        ensure_daemon_with(&self.connect, self.launch.as_mut(), starting)
            .map(|client| Box::new(EngineLink(client)) as Box<dyn Link>)
            .map_err(link_error)
    }
}

struct EngineLink(Client);

impl Link for EngineLink {
    fn requester(&self) -> Option<ConnectionRequester> {
        self.0.hello().requester.clone()
    }

    fn has(&self, capability: &str) -> bool {
        // The client asked for every capability it knows (`connection.accept`): what the
        // daemon serves is what the connection has.
        self.0
            .hello()
            .capabilities
            .as_ref()
            .is_some_and(|served| served.iter().any(|c| c == capability))
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        self.0.call(method, params).map_err(link_error)
    }

    fn call_refusal(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.0.call(method, params).map_err(|err| match err {
            ClientError::Rpc(err) => Refusal::Engine {
                code: err.code,
                data: err.data,
            },
            other => Refusal::Link(link_error(other)),
        })
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<Incoming>, LinkError> {
        Ok(match self.0.next_incoming(timeout).map_err(link_error)? {
            None => None,
            Some(ApiIncoming::Buffered {
                recv_ns,
                notification,
            }) => Some(Incoming::Decoded {
                recv_ns,
                notification,
            }),
            Some(ApiIncoming::Frame { recv_ns, bytes }) => Some(Incoming::Frame { recv_ns, bytes }),
        })
    }
}

fn link_error(err: ClientError) -> LinkError {
    match err {
        ClientError::NotRunning | ClientError::StartTimeout | ClientError::Launch(_) => {
            LinkError::EngineUnavailable
        }
        ClientError::ChannelRejected | ClientError::NotAuthentic => LinkError::Rejected,
        ClientError::Incompatible(_) | ClientError::ClientTooOld(_) => LinkError::Incompatible,
        ClientError::TransportUnsupported | ClientError::Unsupported(_) => LinkError::Unsupported,
        ClientError::Rpc(_) => LinkError::Refused,
        ClientError::Io(_) | ClientError::Protocol(_) => LinkError::Lost,
    }
}
