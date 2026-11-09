//! Review of the repo scope: a repo the engine refuses after it subscribed must not leave its
//! subscription alive at the daemon. The fake cannot refuse `sessions.list`, so the link is
//! wrapped to do it.
#![cfg(unix)]

#[path = "support/fake_daemon.rs"]
mod fake_daemon;

use std::sync::mpsc;
use std::time::Duration;

use fake_daemon::{FakeDaemon, assert_read_only, script_tienda_and_notas};
use gitraptor_api::messages::HelloResult;
use gitraptor_api::rpc::Notification;
use gitraptor_desktop::engine::link::{ConnectFailure, Connector, Link, LinkError};
use gitraptor_desktop::engine::supervisor::{self, HubCmd, Sink};
use gitraptor_desktop::engine::wire::EngineMessage;
use serde_json::{Value, json};

struct RefuseSessions(Box<dyn Link>);

impl Link for RefuseSessions {
    fn hello(&self) -> &HelloResult {
        self.0.hello()
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        if method == "sessions.list" {
            return Err(LinkError::Refused { code: -32602 });
        }
        self.0.call(method, params)
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<(u64, Notification)>, LinkError> {
        self.0.next(timeout)
    }
}

struct Wrapped(Box<dyn Connector>);

impl Connector for Wrapped {
    fn connect(&mut self, start: bool) -> Result<Box<dyn Link>, ConnectFailure> {
        let link = self.0.connect(start)?;
        Ok(Box::new(RefuseSessions(link)))
    }
}

#[test]
fn a_refusal_after_subscribing_cancels_the_subscription() {
    let daemon = FakeDaemon::start();
    daemon.script(script_tienda_and_notas);
    let (tx, rx) = mpsc::channel();
    let handle = supervisor::spawn(
        Box::new(Wrapped(Box::new(daemon.connector()))),
        None,
        Sink::Collect(tx),
    );
    assert!(handle.send(HubCmd::Open {
        repo_id: "tienda".to_owned()
    }));
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(EngineMessage::RepoClosed { repo_id }) if repo_id == "tienda" => break,
            Ok(_) => {}
            Err(e) => panic!("no `repoClosed` after the refused sessions.list: {e}"),
        }
    }
    let subscription = daemon
        .repo_subscription("tienda")
        .expect("the fake subscribed tienda");
    assert_eq!(
        daemon.calls_of("events.unsubscribe"),
        [json!({ "subscription": subscription })],
        "a refused repo cancels the subscription it had taken"
    );
    drop(handle);
    assert_read_only(&daemon.calls());
}
