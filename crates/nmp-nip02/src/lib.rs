//! `nmp-nip02` — follow-list primitives as substrate `ActionModule`s.
//!
//! # Scope
//!
//! This crate owns the NIP-02 kind:3 follow-list write and read surfaces:
//!
//! | Namespace      | Wire kind | NIP    | Verb     |
//! |----------------|-----------|--------|----------|
//! | `nmp.follow`   | kind:3    | NIP-02 | Follow   |
//! | `nmp.unfollow` | kind:3    | NIP-02 | Unfollow |
//!
//! Alongside the action modules and the [`FollowListProjection`] read model,
//! this crate hosts [`ActiveFollowSet`] — the OP-centric home feed's (V-80)
//! follow-set *producer*. It exposes the active account's follows as a live
//! closure predicate (`Arc<dyn Fn(&str) -> bool>`) the generic `RootIndexedFeed`
//! engine in `nmp-feed` consumes, with follow → planner-interest expansion done
//! at the composition root (`nmp-defaults`) — no `FollowSetLookup` trait, no
//! planner `SocialTimeline` seam. See
//! [`active_follow_set`] and ADR-0036.
//!
//! # Why this exists
//!
//! Before this crate, the `Follow` / `Unfollow` `ActionModule`s lived in
//! `apps/chirp/nmp-app-chirp/src/ffi/actions.rs` as app-local verbs. That
//! placement made the wiring app-local even though follow-list edits are
//! generic Nostr protocol primitives.
//!
//! This crate lifts follow/unfollow into a reusable substrate crate. Public
//! NIP-25 reactions now live in `nmp-nip25`; this crate re-exports the old
//! `ReactAction` / `ReactModule` names and its legacy `register_actions`
//! helper delegates to `nmp-nip25` for compatibility.
//!
//! # D0 — namespace hygiene
//!
//! Both NIP-02 namespaces start with `nmp.` (the D9 lint rule for protocol
//! crates). The action executors enqueue follow-list commands and the actor
//! on its own thread builds + signs the kind:3 event (D7).
//!
//! # D11 — single door
//!
//! The bespoke C-ABI symbols `nmp_app_follow` / `nmp_app_unfollow` were
//! deleted in a prior cycle; the only way to reach these verbs from a host is
//! via `nmp_app_dispatch_action(namespace, action_json)`.

use nmp_core::substrate::{ActionModule, ActionRegistrar};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

pub mod active_follow_set;
pub mod projection;
pub mod wire;

pub use active_follow_set::ActiveFollowSet;
pub use nmp_nip25::{ReactAction, ReactModule};
pub use projection::{FollowEntry, FollowListProjection, FollowListSnapshot};
pub use wire::typed_fb::{
    decode_follow_list, encode_follow_list, FILE_IDENTIFIER as FOLLOW_LIST_FILE_IDENTIFIER,
    SCHEMA_ID as FOLLOW_LIST_SCHEMA_ID, SCHEMA_VERSION as FOLLOW_LIST_SCHEMA_VERSION,
};

// ---------------------------------------------------------------------------
// Wire shapes
// ---------------------------------------------------------------------------

/// Wire shape for `nmp.follow` / `nmp.unfollow` —
/// `{"pubkey":"<32-byte hex>"}`.
///
/// Hex-shape validation deliberately stays in the actor's command handlers
/// (which own the user-facing toasts); this struct is a pure JSON-shape
/// decoder. Mirrors the same split the publish engine uses (the registry
/// rejects shape errors, the actor rejects semantic errors with toasts).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PubkeyAction {
    /// Target pubkey in lowercase hex. Validated for hex shape by the
    /// actor's `Follow` / `Unfollow` command handlers (D6 — failures
    /// surface as toasts, never panics).
    pub pubkey: String,
}

/// Wire shape for `nmp.follow_many` — `{"pubkeys":["<hex>", ...]}`.
///
/// Carries the full set of pubkeys to be added to the active account's
/// kind:3 in a single atomic read-modify-write-publish cycle. The
/// actor's `FollowMany` command handler owns validation (per-entry hex
/// shape check, self-exclusion, dedup via idempotent `kind3_tags_after_add`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FollowManyAction {
    /// The set of hex pubkeys to follow. May include duplicates or the
    /// active account's own pubkey — the actor drops them silently.
    pub pubkeys: Vec<String>,
}

// ---------------------------------------------------------------------------
// ActionModule impls
// ---------------------------------------------------------------------------

/// `nmp.follow` — append `pubkey` to the active account's kind:3 follow
/// set and re-publish it.
///
/// The validator is the trait-default no-op accept (the actor's `Follow`
/// command handler owns hex-shape validation + user-facing toasts). The
/// executor enqueues `ActorCommand::Follow` with the registry-minted
/// `correlation_id` so the publish engine's terminal verdict for the
/// kind:3 event lands on the same id the host received from
/// `dispatch_action`.
pub struct FollowModule;

impl ActionModule for FollowModule {
    const NAMESPACE: &'static str = "nmp.follow";
    type Action = PubkeyAction;

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Follow {
            pubkey: action.pubkey,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

/// `nmp.unfollow` — remove `pubkey` from the active account's kind:3
/// follow set and re-publish it.
///
/// Same shape and discipline as [`FollowModule`] — pure shape validator,
/// the actor owns the semantic rules.
pub struct UnfollowModule;

/// `nmp.follow_many` — merge a list of pubkeys into the active account's
/// kind:3 follow set and re-publish it EXACTLY ONCE.
///
/// This is the race-free bulk-follow primitive. Dispatching N sequential
/// `nmp.follow` actions races because each reads kind:3 before the prior
/// signed event is ingested (last-write-wins, silently dropping all but
/// the last follow). `nmp.follow_many` dispatches a single
/// `ActorCommand::FollowMany` which the actor resolves in one
/// read-modify-write-publish cycle on its exclusive execution slot.
pub struct FollowManyModule;

impl ActionModule for UnfollowModule {
    const NAMESPACE: &'static str = "nmp.unfollow";
    type Action = PubkeyAction;

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Unfollow {
            pubkey: action.pubkey,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

impl ActionModule for FollowManyModule {
    const NAMESPACE: &'static str = "nmp.follow_many";
    type Action = FollowManyAction;

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::FollowMany {
            pubkeys: action.pubkeys,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Registration helper
// ---------------------------------------------------------------------------

/// Register only the NIP-02 follow-list `ActionModule`s.
///
/// Registration MUST happen before `nmp_app_start` because
/// the host-side action registrar requires `&mut self`.
pub fn register_follow_actions(app: &mut impl ActionRegistrar) {
    // Yielding defaults (ADR-0049 Part 1): each module installs only if its
    // namespace is unclaimed, so an app may pre-empt any of them regardless of
    // whether it registers before or after `register_defaults`.
    app.register_default_action(FollowModule);
    app.register_default_action(UnfollowModule);
    app.register_default_action(FollowManyModule);
}

/// Compatibility helper for older composition roots that expected
/// `nmp_nip02::register_actions` to wire the full public social bundle.
///
/// New composition code should call [`register_follow_actions`] and
/// `nmp_nip25::register_actions` explicitly so NIP-25 remains the visible
/// owner of public reactions.
pub fn register_actions(app: &mut impl ActionRegistrar) {
    register_follow_actions(app);
    nmp_nip25::register_actions(app);
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::ActorCommand;
    use std::cell::RefCell;

    // ----- namespaces ------------------------------------------------------

    #[test]
    fn follow_namespace_matches_d9_substrate_shape() {
        assert_eq!(FollowModule::NAMESPACE, "nmp.follow");
    }

    #[test]
    fn unfollow_namespace_matches_d9_substrate_shape() {
        assert_eq!(UnfollowModule::NAMESPACE, "nmp.unfollow");
    }

    #[test]
    fn pubkey_action_requires_pubkey_field() {
        // Missing `pubkey` must fail to deserialize so the registry's
        // shape-check rejects the action; the JSON below has the wrong
        // field name and must surface as a serde error (mapped to
        // `ActionRejection::Invalid` by the registry adapter).
        let err = serde_json::from_str::<PubkeyAction>(r#"{"not_pubkey":"x"}"#);
        assert!(err.is_err(), "PubkeyAction must require the `pubkey` field");
    }

    // ----- executor dispatch routing --------------------------------------

    /// The critical contract this crate is meant to enforce: each module's
    /// executor enqueues EXACTLY ONE `ActorCommand`, the variant matches
    /// the verb, the payload threads through verbatim, AND the
    /// registry-minted `correlation_id` is forwarded so the host spinner
    /// closes on the publish engine's terminal verdict.
    fn capture_one(run: impl FnOnce(&dyn Fn(ActorCommand))) -> ActorCommand {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        run(&|cmd| captured.borrow_mut().push(cmd));
        let mut cmds = captured.into_inner();
        assert_eq!(
            cmds.len(),
            1,
            "executor must send exactly one command, got {cmds:?}"
        );
        cmds.pop().unwrap()
    }

    #[test]
    fn follow_executor_enqueues_follow_with_correlation_id() {
        let cmd = capture_one(|send| {
            FollowModule
                .execute(
                    PubkeyAction {
                        pubkey: "deadbeef".to_string(),
                    },
                    "test-cid-follow",
                    send,
                )
                .expect("execute must not fail");
        });
        match cmd {
            ActorCommand::Follow {
                pubkey,
                correlation_id,
            } => {
                assert_eq!(pubkey, "deadbeef");
                assert_eq!(
                    correlation_id.as_deref(),
                    Some("test-cid-follow"),
                    "registry-minted correlation_id must thread through so the host \
                     spinner keyed on the dispatch return value can be cleared"
                );
            }
            other => panic!("expected ActorCommand::Follow, got {other:?}"),
        }
    }

    #[test]
    fn unfollow_executor_enqueues_unfollow_with_correlation_id() {
        let cmd = capture_one(|send| {
            UnfollowModule
                .execute(
                    PubkeyAction {
                        pubkey: "cafebabe".to_string(),
                    },
                    "test-cid-unfollow",
                    send,
                )
                .expect("execute must not fail");
        });
        match cmd {
            ActorCommand::Unfollow {
                pubkey,
                correlation_id,
            } => {
                assert_eq!(pubkey, "cafebabe");
                assert_eq!(correlation_id.as_deref(), Some("test-cid-unfollow"));
            }
            other => panic!("expected ActorCommand::Unfollow, got {other:?}"),
        }
    }
}
