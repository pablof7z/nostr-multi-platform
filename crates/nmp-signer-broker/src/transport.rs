//! Steady-state NIP-46 transport. Used by `Nip46Signer` after handshake to
//! publish kind:24133 RPCs.
//!
//! The signer (in `nmp-signers`) emits `Nip46Rpc` values where
//! `body_json_to_encrypt` is plaintext JSON — see `nmp_signers::signers::nip46`:
//! the signer is transport-agnostic and defers the actual NIP-44
//! encryption + kind:24133 wrapping to whichever `Nip46Transport` impl is
//! plugged in. This module is that impl for the production kernel.
//!
//! Inbound responses are routed back to the signer via
//! `Nip46Signer::deliver_rpc_response`. The broker owns the only
//! `Arc<Nip46Signer>` for the active session; the relay client's event
//! callback walks through `BrokerTransport` to reach it.

use std::sync::{Arc, Mutex, Weak};

use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};
use nmp_signers::Nip46Signer;
use nostr::{EventBuilder, Keys, Kind, PublicKey, Tag, Timestamp};
use serde_json::Value;

use crate::handshake::decode_inbound_response;
use crate::relay_client::RelayClient;

/// Transport that publishes RPCs over the broker's persistent relay client
/// and routes inbound responses to the signer.
pub struct BrokerTransport {
    relay: Arc<dyn RelayClient>,
    local_keys: Keys,
    remote_pubkey: PublicKey,
    /// The signer we feed inbound responses to. `Weak` because the signer's
    /// strong owner is the actor (via `Box<dyn RemoteSignerHandle>`); we
    /// must not extend its lifetime here.
    signer: Mutex<Weak<Nip46Signer>>,
}

impl std::fmt::Debug for BrokerTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokerTransport")
            .field("remote_pubkey", &self.remote_pubkey.to_hex())
            .finish_non_exhaustive()
    }
}

impl BrokerTransport {
    /// Construct without the signer yet (the signer is built from the
    /// transport via `Nip46SignerHandle::complete`, so they're chicken-and-
    /// egg). After constructing the signer, call [`Self::bind_signer`].
    #[must_use]
    pub fn new(
        relay: Arc<dyn RelayClient>,
        local_keys: Keys,
        remote_pubkey: PublicKey,
    ) -> Arc<Self> {
        Arc::new(Self {
            relay,
            local_keys,
            remote_pubkey,
            signer: Mutex::new(Weak::new()),
        })
    }

    /// Wire the signer for inbound dispatch. Called once after the signer is
    /// constructed via `Nip46SignerHandle::complete`.
    pub fn bind_signer(&self, signer: &Arc<Nip46Signer>) {
        if let Ok(mut slot) = self.signer.lock() {
            *slot = Arc::downgrade(signer);
        }
    }

    /// Inbound dispatch: called for every kind:24133 event delivered by the
    /// relay client. Decrypts the content, parses the JSON-RPC envelope,
    /// and forwards `{id, result | error}` to `Nip46Signer::deliver_rpc_response`.
    ///
    /// Public so the broker's relay subscription callback can call this
    /// directly. Idempotent — safe to invoke from multiple frames.
    pub fn dispatch_inbound(&self, event: &Value) {
        let Some(plaintext) = decode_inbound_response(event, &self.local_keys, self.remote_pubkey)
        else {
            return;
        };
        let signer_arc = match self.signer.lock() {
            Ok(guard) => guard.upgrade(),
            Err(_) => return,
        };
        // If the signer has been dropped (account removed, app shutting
        // down) the dispatch is a no-op.
        let Some(signer) = signer_arc else { return };
        use nmp_core::RemoteSignerHandle;
        signer.deliver_rpc_response(&plaintext);
    }
}

impl Nip46Transport for BrokerTransport {
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
        // `rpc.body_json_to_encrypt` is plaintext JSON per the contract in
        // `nmp_signers::signers::nip46` (the signer defers NIP-44 encryption
        // to the transport, which is us). Wrap, encrypt, sign, publish.
        let ciphertext = nostr::nips::nip44::encrypt(
            self.local_keys.secret_key(),
            &self.remote_pubkey,
            rpc.body_json_to_encrypt.as_bytes(),
            nostr::nips::nip44::Version::V2,
        )
        .map_err(|e| SignerError::Backend(format!("nip44 encrypt: {e}")))?;
        let event = EventBuilder::new(Kind::from_u16(24133), ciphertext)
            .tags(vec![Tag::parse(["p", &self.remote_pubkey.to_hex()])
                .map_err(|e| {
                    SignerError::Backend(format!("tag parse: {e}"))
                })?])
            .custom_created_at(Timestamp::from(now_secs()))
            .sign_with_keys(&self.local_keys)
            .map_err(|e| SignerError::Backend(format!("sign event: {e}")))?;
        let serialized = serde_json::to_string(&event)
            .map_err(|e| SignerError::Backend(format!("serialize event: {e}")))?;
        let frame = format!(r#"["EVENT",{serialized}]"#);
        self.relay
            .send(frame)
            .map_err(|e| SignerError::Backend(format!("relay send: {e}")))
    }
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay_client::RelayError;
    use std::sync::Mutex as StdMutex;

    struct CapturingRelay {
        sent: StdMutex<Vec<String>>,
    }
    impl RelayClient for CapturingRelay {
        fn send(&self, frame: String) -> Result<(), RelayError> {
            self.sent.lock().unwrap().push(frame);
            Ok(())
        }
        fn shutdown(&self) {}
    }

    #[test]
    fn send_rpc_emits_kind_24133_event_frame() {
        let local = Keys::generate();
        let remote = Keys::generate().public_key();
        let relay = Arc::new(CapturingRelay {
            sent: StdMutex::new(Vec::new()),
        });
        let transport = BrokerTransport::new(relay.clone() as Arc<dyn RelayClient>, local, remote);
        let rpc = Nip46Rpc {
            id: "abc".to_string(),
            body_json: r#"{"id":"abc","method":"sign_event","params":[]}"#.to_string(),
            body_json_to_encrypt: r#"{"id":"abc","method":"sign_event","params":[]}"#.to_string(),
            relays: vec!["wss://example.com".to_string()],
            remote_pubkey_hex: remote.to_hex(),
        };
        transport.send_rpc(rpc).expect("send ok");

        let sent = relay.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        // Frame is a NIP-01 EVENT envelope.
        assert!(sent[0].starts_with("[\"EVENT\","));
        // The inner event must declare kind=24133 and tag the remote.
        let parsed: Value = serde_json::from_str(&sent[0]).unwrap();
        let inner = &parsed.as_array().unwrap()[1];
        assert_eq!(inner.get("kind").and_then(|v| v.as_u64()), Some(24133));
        let tags = inner.get("tags").and_then(|v| v.as_array()).unwrap();
        assert!(tags.iter().any(|t| t.as_array().is_some_and(|a| a
            .first()
            .and_then(|v| v.as_str())
            == Some("p")
            && a.get(1).and_then(|v| v.as_str()) == Some(&remote.to_hex()))));
    }

    #[test]
    fn send_rpc_surfaces_relay_error_as_backend_error() {
        // A relay that always fails must not let `send_rpc` report success —
        // a dropped sign request must never look like a published one (D6).
        struct FailingRelay;
        impl RelayClient for FailingRelay {
            fn send(&self, _frame: String) -> Result<(), RelayError> {
                Err(RelayError::Disconnected)
            }
            fn shutdown(&self) {}
        }
        let local = Keys::generate();
        let remote = Keys::generate().public_key();
        let transport =
            BrokerTransport::new(Arc::new(FailingRelay) as Arc<dyn RelayClient>, local, remote);
        let rpc = Nip46Rpc {
            id: "abc".to_string(),
            body_json: r#"{"id":"abc"}"#.to_string(),
            body_json_to_encrypt: r#"{"id":"abc","method":"sign_event","params":[]}"#.to_string(),
            relays: vec!["wss://example.com".to_string()],
            remote_pubkey_hex: remote.to_hex(),
        };
        let err = transport.send_rpc(rpc).expect_err("failing relay must error");
        assert!(matches!(err, SignerError::Backend(_)));
    }

    #[test]
    fn dispatch_inbound_without_bound_signer_is_a_silent_noop() {
        // With no signer bound, `dispatch_inbound` must return quietly — not
        // panic — even for a well-formed encrypted event (D6).
        let local = Keys::generate();
        let bunker = Keys::generate();
        let relay = Arc::new(CapturingRelay {
            sent: StdMutex::new(Vec::new()),
        });
        let transport = BrokerTransport::new(
            relay as Arc<dyn RelayClient>,
            local.clone(),
            bunker.public_key(),
        );
        // A genuinely decryptable response event addressed to `local`.
        let rpc = serde_json::json!({"id": "x", "result": "ack"});
        let ct = nostr::nips::nip44::encrypt(
            bunker.secret_key(),
            &local.public_key(),
            rpc.to_string().as_bytes(),
            nostr::nips::nip44::Version::V2,
        )
        .unwrap();
        let event = serde_json::json!({
            "pubkey": bunker.public_key().to_hex(),
            "kind": 24133,
            "content": ct,
        });
        // No `bind_signer` call — must not panic.
        transport.dispatch_inbound(&event);
    }

    #[test]
    fn dispatch_inbound_ignores_malformed_event() {
        // Garbage events must be dropped without panic (D6).
        let local = Keys::generate();
        let remote = Keys::generate().public_key();
        let relay = Arc::new(CapturingRelay {
            sent: StdMutex::new(Vec::new()),
        });
        let transport = BrokerTransport::new(relay as Arc<dyn RelayClient>, local, remote);
        transport.dispatch_inbound(&serde_json::json!({"not": "an event"}));
        transport.dispatch_inbound(&serde_json::Value::Null);
    }
}
