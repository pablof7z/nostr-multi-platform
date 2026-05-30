//! Test-only adapter from app-neutral broker events to actor commands.

use std::sync::mpsc::Sender;
use std::sync::Arc;

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nmp_core::{ActorCommand, RemoteSignerHandle};
use nmp_signer_broker::{BrokerEvent, BunkerBroker};
use nmp_signer_iface::SignerOp;
use nmp_signers::Nip46Signer;

/// Construct a broker that reports events into an actor command channel.
pub fn broker_for_actor(tx: Sender<ActorCommand>) -> Arc<BunkerBroker> {
    BunkerBroker::new(Arc::new(move |event| {
        let _ = tx.send(actor_command_from_event(event));
    }))
}

fn actor_command_from_event(event: BrokerEvent) -> ActorCommand {
    match event {
        BrokerEvent::Progress { stage, message } => {
            ActorCommand::BunkerHandshakeProgress { stage, message }
        }
        BrokerEvent::SignerReady { signer } => ActorCommand::AddRemoteSigner {
            handle: Box::new(ArcRemoteSigner(signer)),
        },
        // V-14 step b: relay-layer connection state. Routes through the actor
        // (D4 — actor is sole writer of the `bunker_connection_state` slot),
        // mirroring the production translation in nmp-ffi/src/signer_broker.rs.
        BrokerEvent::ConnectionStateChanged { state, reason } => {
            ActorCommand::BunkerConnectionStateChanged { state, reason }
        }
    }
}

#[derive(Debug)]
struct ArcRemoteSigner(Arc<Nip46Signer>);

impl RemoteSignerHandle for ArcRemoteSigner {
    fn pubkey_hex(&self) -> String {
        RemoteSignerHandle::pubkey_hex(&*self.0)
    }

    fn signer_kind(&self) -> &'static str {
        RemoteSignerHandle::signer_kind(&*self.0)
    }

    fn persistence_payload_json(&self) -> Option<String> {
        RemoteSignerHandle::persistence_payload_json(&*self.0)
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        RemoteSignerHandle::sign(&*self.0, unsigned)
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        RemoteSignerHandle::nip44_encrypt(&*self.0, recipient_pubkey, plaintext)
    }

    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String> {
        RemoteSignerHandle::nip44_decrypt(&*self.0, sender_pubkey, ciphertext)
    }

    fn deliver_rpc_response(&self, response_json: &str) {
        self.0.ingest_rpc_response(response_json);
    }

    fn disconnect(&self) {
        self.0.drain_pending_with_error("signer disconnected");
    }
}
