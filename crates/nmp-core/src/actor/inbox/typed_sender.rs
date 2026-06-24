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
use crate::actor::{
    ContactsCommand, IdentityCommand, InterestsCommand, LifecycleCommand, RelayCommand,
};

impl CommandSender {
    /// Push a [`crate::planner::LogicalInterest`] into the subscription registry.
    pub fn push_interest(&self, interest: crate::planner::LogicalInterest) {
        let identity = crate::subs::SubIdentity::for_standing_interest(&interest);
        let _ = self.send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
            identity,
            interest,
        }));
    }

    /// Withdraw a previously-pushed interest by id.
    pub fn withdraw_interest(&self, id: crate::planner::InterestId) {
        let identity = crate::subs::SubIdentity::for_standing_interest_id(
            id,
            crate::subs::SubScope::Global,
        );
        let _ = self.send(ActorCommand::Interests(InterestsCommand::DropInterestOwner(
            identity,
        )));
    }

    /// Mark the kernel dirty so snapshot projections re-emit next tick.
    pub fn mark_changed_since_emit(&self) {
        let _ = self.send(ActorCommand::Lifecycle(
            LifecycleCommand::MarkChangedSinceEmit,
        ));
    }

    /// Tear down the active-follows feed declaration and withdraw its interests.
    pub fn clear_active_follows_feed(&self) {
        let _ = self.send(ActorCommand::Contacts(
            ContactsCommand::ClearActiveFollowsFeed,
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
}
