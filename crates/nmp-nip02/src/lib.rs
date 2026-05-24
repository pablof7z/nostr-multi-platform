//! `nmp-nip02` — social-graph primitives as substrate `ActionModule`s.
//!
//! # Scope
//!
//! Despite the crate name (NIP-02 = Follow List, kind:3), this crate hosts
//! the minimal cluster of social verbs every Nostr client implements:
//!
//! | Namespace            | Wire kind | NIP    | Verb           |
//! |----------------------|-----------|--------|----------------|
//! | `nmp.follow`         | kind:3    | NIP-02 | Follow         |
//! | `nmp.unfollow`       | kind:3    | NIP-02 | Unfollow       |
//! | `nmp.nip25.react`    | kind:7    | NIP-25 | Reaction       |
//!
//! NIP-02 (follow list) and NIP-25 (reactions) are co-located here because
//! they share the same governance story — they are substrate-level social
//! primitives that escape through `nmp_app_dispatch_action`, not per-app
//! verbs. Keeping them in one crate keeps the action surface coherent and
//! gives every Nostr app on top of NMP a single `register_actions(app)` call
//! to wire the social graph.
//!
//! # Why this exists
//!
//! Before this crate, the `Follow` / `Unfollow` / `React` `ActionModule`s
//! lived in `apps/chirp/nmp-app-chirp/src/ffi/actions.rs` (as
//! `ChirpFollowModule` / `ChirpUnfollowModule` / `ChirpReactModule`). That
//! placement made the wiring app-local even though the verbs themselves are
//! generic Nostr protocol primitives — Opus direction review #10 flagged
//! this as the Follow / React "escape path" out of `nmp-app-chirp`. Any
//! future Nostr app on top of NMP would have to either depend on the Chirp
//! app crate (an inversion of the dep graph) or re-implement the modules
//! verbatim.
//!
//! This crate lifts the three modules into a reusable substrate crate so
//! any app calls `nmp_nip02::register_actions(&mut app)` to wire the
//! social-graph dispatch namespaces — the same shape `nmp-nip17` /
//! `nmp-nip57` / `nmp-nip65` already use.
//!
//! # D0 — namespace hygiene
//!
//! All three namespaces start with `nmp.` (the D9 lint rule for protocol
//! crates), and the kernel still carries no NIP-02 / NIP-25 nouns: the
//! executors enqueue the existing `ActorCommand::{React, Follow, Unfollow}`
//! variants, and the actor on its own thread builds + signs the kind:3 /
//! kind:7 event (D7 — sign on the actor thread).
//!
//! # D11 — single door
//!
//! The bespoke C-ABI symbols `nmp_app_react` / `nmp_app_follow` /
//! `nmp_app_unfollow` were deleted in a prior cycle; the only way to reach
//! these verbs from a host is via `nmp_app_dispatch_action(namespace,
//! action_json)`. This crate plugs into the same registry that the publish
//! engine, NIP-17, NIP-57, NIP-65, and NIP-29 use.

use nmp_core::substrate::ActionModule;
use nmp_core::{ActorCommand, NmpApp};
use serde::{Deserialize, Serialize};

pub mod projection;

pub use projection::{FollowEntry, FollowListProjection};

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

/// Wire shape for `nmp.nip25.react` —
/// `{"target_event_id":"<hex>","reaction":"<emoji-or-+>"}`.
///
/// `reaction` defaults to `"+"` (the standard kind:7 "like") when absent,
/// matching the behaviour of the deleted `nmp_app_react` C symbol's
/// `.unwrap_or("+")` so a host migrating from the bespoke symbol gets the
/// same defaults.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReactAction {
    /// Hex id of the event being reacted to.
    pub target_event_id: String,
    /// Reaction content. NIP-25 allows `"+"` (like), `"-"` (dislike), or
    /// an arbitrary emoji shortcode. Defaults to `"+"` when absent.
    #[serde(default = "default_reaction")]
    pub reaction: String,
}

fn default_reaction() -> String {
    "+".to_string()
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

impl ActionModule for UnfollowModule {
    const NAMESPACE: &'static str = "nmp.unfollow";
    type Action = PubkeyAction;

    fn execute(
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

/// `nmp.nip25.react` — publish a kind:7 NIP-25 reaction to the event
/// identified by `target_event_id`.
///
/// The executor enqueues `ActorCommand::React`; the actor builds + signs
/// the kind:7 event on its own thread (D7) and the publish engine reports
/// the terminal verdict under the same `correlation_id` the host received
/// from `dispatch_action`.
pub struct ReactModule;

impl ActionModule for ReactModule {
    const NAMESPACE: &'static str = "nmp.nip25.react";
    type Action = ReactAction;

    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::React {
            target_event_id: action.target_event_id,
            reaction: action.reaction,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Registration helper
// ---------------------------------------------------------------------------

/// Register all three social-graph `ActionModule`s against `app`'s action
/// registry. This is the single call a host wires from its init path
/// (mirrors `nmp_nip17::register_actions`, `nmp_nip57::register_actions`,
/// `nmp_router::register_actions` — the NIP-65 publish action, absorbed
/// from the former `nmp-nip65` crate at step 3 of the crate-boundary
/// migration).
///
/// Registration MUST happen before `nmp_app_start` because
/// `NmpApp::register_action` requires `&mut self`.
pub fn register_actions(app: &mut NmpApp) {
    app.register_action::<FollowModule>();
    app.register_action::<UnfollowModule>();
    app.register_action::<ReactModule>();
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
    fn react_namespace_matches_d9_substrate_shape() {
        // NIP-25 is the reactions NIP; the namespace carries the NIP
        // marker so a reviewer can map verb → NIP at a glance.
        assert_eq!(ReactModule::NAMESPACE, "nmp.nip25.react");
    }

    // ----- decoder defaults ------------------------------------------------

    #[test]
    fn react_action_defaults_reaction_to_plus_when_absent() {
        let action: ReactAction =
            serde_json::from_str(r#"{"target_event_id":"abc"}"#).expect("valid JSON");
        assert_eq!(
            action.reaction, "+",
            "absent reaction must default to '+' to match the deleted \
             nmp_app_react C symbol's .unwrap_or(\"+\") behaviour"
        );
        assert_eq!(action.target_event_id, "abc");
    }

    #[test]
    fn react_action_preserves_explicit_reaction() {
        let action: ReactAction =
            serde_json::from_str(r#"{"target_event_id":"abc","reaction":"🤙"}"#)
                .expect("valid JSON");
        assert_eq!(action.reaction, "🤙");
    }

    #[test]
    fn pubkey_action_requires_pubkey_field() {
        // Missing `pubkey` must fail to deserialize so the registry's
        // shape-check rejects the action; the JSON below has the wrong
        // field name and must surface as a serde error (mapped to
        // `ActionRejection::Invalid` by the registry adapter).
        let err = serde_json::from_str::<PubkeyAction>(r#"{"not_pubkey":"x"}"#);
        assert!(
            err.is_err(),
            "PubkeyAction must require the `pubkey` field"
        );
    }

    // ----- executor dispatch routing --------------------------------------

    /// The critical contract this crate is meant to enforce: each module's
    /// executor enqueues EXACTLY ONE `ActorCommand`, the variant matches
    /// the verb, the payload threads through verbatim, AND the
    /// registry-minted `correlation_id` is forwarded so the host spinner
    /// closes on the publish engine's terminal verdict.
    fn capture_one(
        run: impl FnOnce(&dyn Fn(ActorCommand)),
    ) -> ActorCommand {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        run(&|cmd| captured.borrow_mut().push(cmd));
        let mut cmds = captured.into_inner();
        assert_eq!(cmds.len(), 1, "executor must send exactly one command, got {cmds:?}");
        cmds.pop().unwrap()
    }

    #[test]
    fn follow_executor_enqueues_follow_with_correlation_id() {
        let cmd = capture_one(|send| {
            FollowModule::execute(
                PubkeyAction { pubkey: "deadbeef".to_string() },
                "test-cid-follow",
                send,
            )
            .expect("execute must not fail");
        });
        match cmd {
            ActorCommand::Follow { pubkey, correlation_id } => {
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
            UnfollowModule::execute(
                PubkeyAction { pubkey: "cafebabe".to_string() },
                "test-cid-unfollow",
                send,
            )
            .expect("execute must not fail");
        });
        match cmd {
            ActorCommand::Unfollow { pubkey, correlation_id } => {
                assert_eq!(pubkey, "cafebabe");
                assert_eq!(correlation_id.as_deref(), Some("test-cid-unfollow"));
            }
            other => panic!("expected ActorCommand::Unfollow, got {other:?}"),
        }
    }

    #[test]
    fn react_executor_enqueues_react_with_payload_and_correlation_id() {
        let cmd = capture_one(|send| {
            ReactModule::execute(
                ReactAction {
                    target_event_id: "abc".to_string(),
                    reaction: "+".to_string(),
                },
                "test-cid-react",
                send,
            )
            .expect("execute must not fail");
        });
        match cmd {
            ActorCommand::React {
                target_event_id,
                reaction,
                correlation_id,
            } => {
                assert_eq!(target_event_id, "abc");
                assert_eq!(reaction, "+");
                assert_eq!(correlation_id.as_deref(), Some("test-cid-react"));
            }
            other => panic!("expected ActorCommand::React, got {other:?}"),
        }
    }
}
