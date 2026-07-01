//! The `ActionModule` impls for the NIP-02 follow-list verbs
//! (`nmp.follow` / `nmp.unfollow` / `nmp.follow_many`).
//!
//! Extracted from `lib.rs` to keep that file under the 500-LOC hand-authored
//! ceiling (AGENTS.md / V-12) after the ADR-0064 / S3 (#1751) typed-payload
//! `decode_payload` overrides landed. The public `*Module` structs and the
//! action shapes stay in `lib.rs`; this file holds only the trait impls.
//!
//! Each impl opts into the typed FlatBuffers payload doorway via
//! `decode_payload`, delegating to the crate's `ActionPayload` codec
//! (`wire/action_payload.rs`) — the fail-closed `schema_version` gate runs in
//! `decode`, BEFORE `start()`.

use nmp_core::actor::ActorCommand;
use nmp_core::actor::ContactsCommand;
use nmp_core::substrate::{ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError};

use crate::{FollowManyAction, FollowManyModule, FollowModule, PubkeyAction, UnfollowModule};

impl ActionModule for FollowModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework("nmp.follow", "action.nmp.follow");
    type Action = PubkeyAction;

    /// ADR-0064 / S3: opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PubkeyAction as ActionPayload>::decode(bytes))
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Contacts(ContactsCommand::Follow {
            pubkey: action.pubkey,
            correlation_id: Some(correlation_id.to_string()),
        }));
        Ok(())
    }
}

impl ActionModule for UnfollowModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.unfollow",
            "action.nmp.unfollow",
        );
    type Action = PubkeyAction;

    /// ADR-0064 / S3: opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PubkeyAction as ActionPayload>::decode(bytes))
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Contacts(ContactsCommand::Unfollow {
            pubkey: action.pubkey,
            correlation_id: Some(correlation_id.to_string()),
        }));
        Ok(())
    }
}

impl ActionModule for FollowManyModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.follow_many",
            "action.nmp.follow_many",
        );
    type Action = FollowManyAction;

    /// ADR-0064 / S3: opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<FollowManyAction as ActionPayload>::decode(bytes))
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Contacts(ContactsCommand::FollowMany {
            pubkeys: action.pubkeys,
            correlation_id: Some(correlation_id.to_string()),
        }));
        Ok(())
    }
}
