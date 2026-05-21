//! Chirp social-verb `ActionModule` impls.
//!
//! ADR-0027 — each Chirp social verb (`chirp.react`, `chirp.follow`,
//! `chirp.unfollow`) is a typed [`nmp_core::substrate::ActionModule`] impl.
//! The validator half (`start`) checks the JSON shape; the executor half
//! (`execute`) enqueues the matching [`nmp_core::ActorCommand`]. Before
//! ADR-0027 these lived as inline anonymous closures in `ffi.rs`; promoting
//! them to typed impls means partial registration is now a compile error.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

/// `chirp.react` action body: `{"target_event_id":"<hex>","reaction":"+"}`.
/// `reaction` defaults to `"+"` (the standard kind:7 like) when absent —
/// matching the old `nmp_app_react` FFI symbol's `unwrap_or("+")` behaviour.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ChirpReactInput {
    pub target_event_id: String,
    #[serde(default = "default_reaction")]
    pub reaction: String,
}

fn default_reaction() -> String {
    "+".to_string()
}

/// `chirp.follow` / `chirp.unfollow` action body: `{"pubkey":"<hex>"}`.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ChirpPubkeyInput {
    pub pubkey: String,
}

/// Typed [`ActionModule`] for `chirp.react` — kind:7 reaction.
pub struct ChirpReactModule;
impl ActionModule for ChirpReactModule {
    const NAMESPACE: &'static str = "chirp.react";
    type Action = ChirpReactInput;

    fn start(_ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
        // Shape validation is the serde decode at the adapter boundary;
        // hex-shape validation lives in the actor's command handler (D6).
        Ok(())
    }

    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::React {
            target_event_id: action.target_event_id,
            reaction: action.reaction,
        });
        Ok(())
    }
}

/// Typed [`ActionModule`] for `chirp.follow` — append `pubkey` to the active
/// account's kind:3 set.
pub struct ChirpFollowModule;
impl ActionModule for ChirpFollowModule {
    const NAMESPACE: &'static str = "chirp.follow";
    type Action = ChirpPubkeyInput;

    fn start(_ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
        Ok(())
    }

    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Follow { pubkey: action.pubkey });
        Ok(())
    }
}

/// Typed [`ActionModule`] for `chirp.unfollow` — remove `pubkey` from the
/// kind:3 set.
pub struct ChirpUnfollowModule;
impl ActionModule for ChirpUnfollowModule {
    const NAMESPACE: &'static str = "chirp.unfollow";
    type Action = ChirpPubkeyInput;

    fn start(_ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
        Ok(())
    }

    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Unfollow { pubkey: action.pubkey });
        Ok(())
    }
}
