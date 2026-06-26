//! Typed dispatch methods for [`super::CommandSender`] (#1721 slice 3b-iii).
//!
//! Extracted from `inbox.rs` to keep that file under the 500 LOC hard cap
//! (AGENTS.md). Callers use these instead of constructing [`super::ActorCommand`]
//! variants directly.
//!
//! Each method is a thin, fire-and-forget wrapper: it builds the correct variant
//! and calls `send`, discarding the error with `let _ =` (D6 — a closed inbox
//! is a silent no-op, identical to the callers' prior pattern of
//! `let _ = sender.send(ActorCommand::…)`).

use super::CommandSender;
use crate::actor::ActorCommand;
use crate::actor::{IdentityCommand, InterestsCommand, LifecycleCommand, RelayCommand};

impl CommandSender {
    /// Attach one scoped owner to a [`crate::planner::LogicalInterest`].
    pub fn ensure_interest(
        &self,
        identity: crate::subs::SubIdentity,
        interest: crate::planner::LogicalInterest,
    ) {
        let _ = self.send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
            identity,
            interest,
        }));
    }

    /// Detach one scoped owner from the subscription registry.
    pub fn drop_interest_owner(&self, identity: crate::subs::SubIdentity) {
        let _ = self.send(ActorCommand::Interests(
            InterestsCommand::DropInterestOwner(identity),
        ));
    }

    /// Mark the kernel dirty so snapshot projections re-emit next tick.
    pub fn mark_changed_since_emit(&self) {
        let _ = self.send(ActorCommand::Lifecycle(
            LifecycleCommand::MarkChangedSinceEmit,
        ));
    }

    /// Detach one owner from an interest opened via `OpenInterest`.
    /// `relay_pin` is always `None` for the standard outbox-routed path.
    pub fn close_interest(&self, filter_json: String, consumer_id: String, scope: u32) {
        let _ = self.send(ActorCommand::Interests(InterestsCommand::CloseInterest {
            filter_json,
            consumer_id,
            scope,
            relay_pin: None,
        }));
    }

    /// Store a fetched NIP-11 relay-information document on the kernel row.
    pub fn set_relay_info(&self, relay_url: String, doc_json: String) {
        let _ = self.send(ActorCommand::Relay(RelayCommand::SetRelayInfo {
            relay_url,
            doc_json,
        }));
    }

    /// Deliver an inbound remote-signer response for correlation-keyed dispatch.
    pub fn deliver_signer_response(&self, response_json: String) {
        let _ = self.send(ActorCommand::Identity(
            IdentityCommand::DeliverSignerResponse { response_json },
        ));
    }

    /// Add a signer from one of the [`crate::SignerSource`] variants.
    pub fn add_signer(&self, source: crate::SignerSource, make_active: bool) {
        let _ = self.send(ActorCommand::Identity(IdentityCommand::AddSigner {
            source,
            make_active,
        }));
    }

    /// Report an app-lifecycle phase transition to the actor.
    pub fn lifecycle_event(&self, phase: crate::kernel::LifecyclePhase) {
        let _ = self.send(ActorCommand::Lifecycle(LifecycleCommand::LifecycleEvent(
            phase,
        )));
    }

    /// Request clean actor shutdown.
    pub fn shutdown(&self) {
        let _ = self.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown));
    }

    /// Update the NIP-55 external-signer health state in the kernel.
    pub fn nip55_signer_state_changed(&self, state: String, reason: Option<String>) {
        let _ = self.send(ActorCommand::Identity(
            IdentityCommand::Nip55SignerStateChanged { state, reason },
        ));
    }

    /// Report a NIP-46 bunker handshake progress event.
    pub fn bunker_handshake_progress(
        &self,
        stage: String,
        code: Option<String>,
        message: Option<String>,
    ) {
        let _ = self.send(ActorCommand::Identity(
            IdentityCommand::BunkerHandshakeProgress {
                stage,
                code,
                message,
            },
        ));
    }

    /// Report a NIP-46 bunker relay-layer connection state change.
    pub fn bunker_connection_state_changed(&self, state: String, reason: Option<String>) {
        let _ = self.send(ActorCommand::Identity(
            IdentityCommand::BunkerConnectionStateChanged { state, reason },
        ));
    }

    /// Fire-and-forget outbound frame send (ADR-0065 `EnqueueOutbound`).
    ///
    /// Posts a raw `text` frame to `relay_url` on the `role` lane without
    /// waiting for delivery confirmation. Used by `RelayConnectedHook` impls
    /// (e.g. `nmp-nip46-runtime`) that hold a `CommandSender` but cannot
    /// return `Vec<OutboundMessage>` directly.
    pub fn enqueue_outbound(
        &self,
        role: nmp_network::role::RelayRole,
        relay_url: String,
        text: String,
    ) {
        let _ = self.send(ActorCommand::EnqueueOutbound { role, relay_url, text });
    }
}
