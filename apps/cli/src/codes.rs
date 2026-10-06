//! Message keys of the contract's typed codes (ADR-CKP-003 § 4 N7, V8):
//! a client presents a code and its `data`, never the daemon's `message`.
//! Each code has a key `<group>.<wire text>` in both catalogs; the test
//! below keeps a new variant from shipping without its messages.

use gitraptor_api::Actor;
#[cfg(test)]
use gitraptor_api::catalog::Layer;
use gitraptor_api::rpc::ErrorCode;
use gitraptor_api::scope::{AutostartView, ConnectionRequester};
use serde::Serialize;

use crate::i18n::t;
use crate::status;

/// `<group>.<wire text of value>`.
pub fn key(group: &str, value: &impl Serialize) -> String {
    format!("{group}.{}", status::wire(value))
}

/// The message of an error code (`error.<name>`).
pub fn error_key(code: ErrorCode) -> String {
    format!("error.{}", code.as_str())
}

fn actor_text(actor: &Actor) -> String {
    match actor {
        Actor::Unattributed => t("actor.unattributed", &[]),
        Actor::Agent { kind, name, .. } => t(
            &key("requester.actor", kind),
            &[(
                "name",
                &name.as_ref().map(|n| n.sanitized()).unwrap_or_default(),
            )],
        ),
    }
}

/// "You act as X (layer Y)" for the handshake's requester (N5).
pub fn requester_text(requester: &ConnectionRequester) -> String {
    match requester {
        ConnectionRequester::Resolved { actor, layer, .. } => t(
            "daemon.status.requester",
            &[
                ("actor", &actor_text(actor)),
                ("layer", &t(&key("layer", layer), &[])),
            ],
        ),
        ConnectionRequester::Unverified => t("daemon.status.requester-unverified", &[]),
    }
}

pub fn autostart_text(autostart: AutostartView) -> String {
    t(
        "daemon.status.autostart",
        &[("state", &t(&key("autostart", &autostart), &[]))],
    )
}

/// Every layer, for the catalog test.
#[cfg(test)]
const LAYERS: [Layer; 2] = [Layer::Cockpit, Layer::Mcp];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::has_key;
    use gitraptor_api::AgentKind;
    use gitraptor_api::messages::{ResyncReason, StopCauseCode};
    use gitraptor_api::rpc::{InvalidReason, ScopeRefusal};
    use gitraptor_api::scope::UnavailableCause;

    /// V8: every code of the contract that a client presents has its
    /// message in English and Spanish.
    #[test]
    fn every_contract_code_has_both_messages() {
        let mut keys: Vec<String> = ErrorCode::ALL.into_iter().map(error_key).collect();
        keys.extend(ResyncReason::ALL.iter().map(|v| key("resync", v)));
        keys.extend(UnavailableCause::ALL.iter().map(|v| key("unavailable", v)));
        keys.extend(AutostartView::ALL.iter().map(|v| key("autostart", v)));
        keys.extend(LAYERS.iter().map(|v| key("layer", v)));
        keys.extend(StopCauseCode::ALL.iter().map(|v| key("stop", v)));
        keys.extend(InvalidReason::ALL.iter().map(|v| key("invalid", v)));
        keys.extend(ScopeRefusal::ALL.iter().map(|v| key("scope", v)));
        keys.extend(
            [AgentKind::ClaudeCode, AgentKind::Other]
                .iter()
                .map(|v| key("requester.actor", v)),
        );
        keys.extend(
            [
                "actor.unattributed",
                "daemon.status.requester",
                "daemon.status.requester-unverified",
                "daemon.status.autostart",
                "channel.rejected",
            ]
            .map(String::from),
        );
        let missing: Vec<_> = keys.iter().filter(|k| !has_key(k)).collect();
        assert!(missing.is_empty(), "missing messages: {missing:?}");
    }

    #[test]
    fn requester_and_autostart_are_presented_from_codes() {
        let text = requester_text(&ConnectionRequester::Resolved {
            actor: Actor::Agent {
                kind: AgentKind::Other,
                name: Some(gitraptor_api::UntrustedName::new("bot\u{1b}]0;x\u{7}")),
                origin: gitraptor_api::AgentOrigin::Registered,
            },
            layer: Layer::Mcp,
            confirmable: false,
        });
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(text.contains("bot"), "{text}");
        assert!(!text.contains("daemon.status"), "{text}");
        assert!(!autostart_text(AutostartView::Unknown).contains("autostart."));
    }
}
