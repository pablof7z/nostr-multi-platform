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
//! at the composition root (`explicit composition`) — no `FollowSetLookup` trait, no
//! planner `SocialTimeline` seam. See
//! [`active_follow_set`] and ADR-0036.
//!
//! # Why this exists
//!
//! Before this crate, the `Follow` / `Unfollow` `ActionModule`s lived in
//! `apps/chirp/crates/nmp-app-chirp/src/ffi/actions.rs` as app-local verbs. That
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
//! Both NIP-02 namespaces start with `nmp.` (the action_namespace lint rule
//! for protocol crates). The action executors enqueue follow-list commands and
//! the actor on its own thread builds + signs the kind:3 event (D7).
//!
//! # D11 — single door
//!
//! The bespoke C-ABI symbols `nmp_app_follow` / `nmp_app_unfollow` were
//! deleted in a prior cycle; the only way to reach these verbs from a host is
//! via `nmp_app_dispatch_action(namespace, action_json)`.

use std::sync::Arc;

// `ActionModule` is re-exported into the `tests` module's `use super::*;` glob
// (the namespace + executor tests call `FollowModule::NAMESPACE` / `.execute`).
// The `ActionPayload` typed-decode impls live in `action_modules.rs`.
use nmp_core::actor::ActorCommand;
use nmp_core::actor::InterestsCommand;
#[cfg_attr(not(test), allow(unused_imports))]
use nmp_core::substrate::ActionModule;
use nmp_core::substrate::{
    ActionRegistrar, HostCapabilities, IdentityChangeRegistrar, SnapshotProjectionRegistrar,
};
use serde::{Deserialize, Serialize};

// The `ActionModule` impls for the three follow verbs (split out to keep this
// file under the 500-LOC ceiling after the S3 typed-payload overrides).
mod action_modules;
pub mod active_follow_set;
mod latest_kind3;
pub mod projection;
pub mod wire;

pub use active_follow_set::ActiveFollowSet;
pub use latest_kind3::LatestKind3FollowSet;
pub use nmp_nip25::{ReactAction, ReactModule};
pub use projection::{FollowEntry, FollowListProjection, FollowListSnapshot};
pub use wire::typed_fb::{
    decode_follow_list, encode_follow_list, FILE_IDENTIFIER as FOLLOW_LIST_FILE_IDENTIFIER,
    SCHEMA_ID as FOLLOW_LIST_SCHEMA_ID, SCHEMA_VERSION as FOLLOW_LIST_SCHEMA_VERSION,
};

const FOLLOW_LIST_PROJECTION_KEY: nmp_ownership::DeclaredProjectionKey =
    nmp_ownership::DeclaredProjectionKey::framework(
        "nmp.follow_list",
        "projection.nmp.follow_list",
    );

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

// The `ActionModule` impls for these three structs (incl. the ADR-0064 / S3
// typed-payload `decode_payload` overrides) live in `action_modules.rs` — split
// out to keep this file under the 500-LOC ceiling.

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
    // whether it registers before or after `explicit owner composition`.
    app.register_default_action(FollowModule);
    app.register_default_action(UnfollowModule);
    app.register_default_action(FollowManyModule);
}

/// Compatibility helper for older composition roots that expected
/// `nmp_nip02::register_actions` to wire the full public social bundle.
///
/// New composition code should call [`register_follow_actions`] and
/// `nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app)`
/// explicitly so NIP-25 remains the visible owner of public reactions.
pub fn register_actions(app: &mut impl ActionRegistrar) {
    register_follow_actions(app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app);
}

/// Wire the NIP-02 follow-list read runtime into `app`.
///
/// Registers the `"nmp.follow_list"` typed FlatBuffers snapshot projection
/// (ADR-0037) backed by the canonical event store — the single source of truth
/// for the active account's kind:3 follow set — and enqueues a `{"kinds":[3]}`
/// kind:3 interest so the kernel's cache-serve path populates the store before
/// the first snapshot tick.
///
/// # Why this fixes the Follow button
///
/// The prior `nmp_app_chirp_register_follow_list` registered a
/// `ObservedProjectionSink` that kept a LOCAL `HashMap` of follows. This missed
/// the startup cache-serve (runs before the lazy observer exists) so
/// already-followed accounts appeared as "Follow" on cold start. This function
/// replaces that approach: the projection is a PURE READ over the event store;
/// the kernel's demand interest drives acquisition through the standard
/// cache-serve path.
///
/// # Interest lifecycle
///
/// On initial call (with an active account) and on each subsequent account
/// change, `register_follow_state_runtime` enqueues
/// `ActorCommand::Interests(InterestsCommand::OpenInterest { filter_json: {"kinds":[3],"authors":[<pubkey>]},
/// consumer_id: "nmp.nip02.follow_list", scope: 0 })`. The actor routes this
/// through `Kernel::register_interest`, which mutates the registry, enqueues a
/// cache-serve for the active account's kind:3, and triggers a compile
/// invalidation — the same front-door every other interest uses.
///
/// On account switch the old interest is closed (the account-change observer
/// enqueues `ActorCommand::CloseInterest` for the previous pubkey) and a new
/// one is opened for the incoming pubkey.
///
/// # Wire shape
///
/// The `"nmp.follow_list"` snapshot key and the `"nmp.nip02.follow_list"`
/// schema id are preserved — no Swift decoder changes are needed.
///
/// # Called from Chirp FFI
///
/// `nmp_app_chirp_register_follow_list` calls this function instead of
/// constructing a `FollowListProjection` directly. The `active_pubkey` C
/// string parameter is no longer used to seed a local slot; the projection
/// reads the kernel's canonical active-account slot via `app.active_pubkey()`.
pub fn register_follow_state_runtime(
    app: &(impl HostCapabilities + IdentityChangeRegistrar + SnapshotProjectionRegistrar),
) {
    use crate::wire::typed_fb;

    let active_pubkey = app.active_pubkey();
    let latest_kind3 = LatestKind3FollowSet::new(app.event_store_handle());
    let tx = app.actor_sender();

    let projection = Arc::new(crate::projection::FollowListProjection::new(
        Arc::clone(&active_pubkey),
        latest_kind3,
    ));

    // --- Interest registration helper ---
    // Enqueues `OpenInterest` (kind:3, authors:[pubkey]) on the actor channel
    // so the kernel's cache-serve pipeline populates the event store before the
    // first snapshot tick. Uses the `scope: 0` (ActiveAccount) convention.
    // D8: channel send is non-blocking, bounded to one command.
    const CONSUMER_ID: &str = "nmp.nip02.follow_list";

    let open_interest = {
        let tx = tx.clone();
        move |pubkey: &str| {
            let _ = tx.send(ActorCommand::Interests(InterestsCommand::OpenInterest {
                filter_json: format!(r#"{{"kinds":[3],"authors":["{pubkey}"]}}"#),
                consumer_id: CONSUMER_ID.to_string(),
                scope: 0, // ActiveAccount
            }));
        }
    };

    let close_interest = {
        let tx = tx.clone();
        move |pubkey: &str| {
            let _ = tx.send(ActorCommand::Interests(InterestsCommand::CloseInterest {
                filter_json: format!(r#"{{"kinds":[3],"authors":["{pubkey}"]}}"#),
                consumer_id: CONSUMER_ID.to_string(),
                scope: 0,
                relay_pin: None,
            }));
        }
    };

    // If there is already an active account, open the interest immediately so
    // the kernel serves the cached kind:3 before the first snapshot tick.
    {
        let maybe_pubkey = active_pubkey.lock().ok().and_then(|g| g.clone());
        if let Some(pubkey) = maybe_pubkey {
            open_interest(&pubkey);
        }
    }

    // On each account change: close the old interest, open a new one.
    // The identity-change observer fires on the update-listener thread after
    // the kernel writes `active_pubkey` — no race with the slot read above.
    //
    // `last_pubkey` tracks the previously-active account so we can close its
    // specific interest (the filter_json includes the author, so we must close
    // with the OLD pubkey, not the new one). Seeded from the slot's current
    // value so the first fire is not a false positive.
    let last_pubkey = {
        let slot = Arc::clone(&active_pubkey);
        Arc::new(std::sync::Mutex::new(
            slot.lock().ok().and_then(|g| g.clone()),
        ))
    };

    app.register_identity_change_observer(move |new_pubkey: Option<String>| {
        // Close the old interest, if any.
        if let Ok(mut prev) = last_pubkey.lock() {
            if let Some(old) = prev.take() {
                close_interest(&old);
            }
            // Open a new interest for the incoming account.
            if let Some(ref pubkey) = new_pubkey {
                open_interest(pubkey);
            }
            *prev = new_pubkey;
        }
    });

    // Typed FlatBuffers sidecar (ADR-0037): PURE READ — reads the event store
    // and encodes. Wire shape is preserved: key = "nmp.follow_list",
    // schema_id = "nmp.nip02.follow_list". D8: non-blocking lookup.
    let projection_typed = Arc::clone(&projection);
    app.register_typed_snapshot_projection(FOLLOW_LIST_PROJECTION_KEY, move || {
        let snapshot = projection_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.follow_list".to_string(),
            schema_id: FOLLOW_LIST_SCHEMA_ID.to_string(),
            schema_version: FOLLOW_LIST_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(FOLLOW_LIST_FILE_IDENTIFIER).into_owned(),
            payload: typed_fb::encode_follow_list(&snapshot),
            ..Default::default()
        })
    });
}

/// Build the typed `"nmp.follow_list"` sidecar entry from a live
/// [`FollowListProjection`].
///
/// Always emits (parity with the generic projection, which always contributes
/// `{"follows":[]}`): no active account yields an empty typed buffer.
///
/// The registration KEY is `"nmp.follow_list"` (matching the generic
/// projection's namespace); the typed payload's `schema_id` is the distinct
/// `"nmp.nip02.follow_list"`. Exposed here so platform-specific tests can
/// verify the sidecar shape without duplicating the encoding.
#[must_use]
pub fn typed_projection_entry(
    proj: &projection::FollowListProjection,
) -> Option<nmp_core::TypedProjectionData> {
    use crate::wire::typed_fb;
    let snapshot = proj.snapshot();
    Some(nmp_core::TypedProjectionData {
        key: "nmp.follow_list".to_string(),
        schema_id: FOLLOW_LIST_SCHEMA_ID.to_string(),
        schema_version: FOLLOW_LIST_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(FOLLOW_LIST_FILE_IDENTIFIER).into_owned(),
        payload: typed_fb::encode_follow_list(&snapshot),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::actor::{ActorCommand, ContactsCommand};
    use std::cell::RefCell;

    // ----- namespaces ------------------------------------------------------

    #[test]
    fn follow_namespace_matches_action_namespace_substrate_shape() {
        assert_eq!(FollowModule::NAMESPACE, "nmp.follow");
    }

    #[test]
    fn unfollow_namespace_matches_action_namespace_substrate_shape() {
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
                    &nmp_core::substrate::ActionContext::default(),
                    PubkeyAction {
                        pubkey: "deadbeef".to_string(),
                    },
                    "test-cid-follow",
                    send,
                )
                .expect("execute must not fail");
        });
        match cmd {
            ActorCommand::Contacts(ContactsCommand::Follow {
                pubkey,
                correlation_id,
            }) => {
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
                    &nmp_core::substrate::ActionContext::default(),
                    PubkeyAction {
                        pubkey: "cafebabe".to_string(),
                    },
                    "test-cid-unfollow",
                    send,
                )
                .expect("execute must not fail");
        });
        match cmd {
            ActorCommand::Contacts(ContactsCommand::Unfollow {
                pubkey,
                correlation_id,
            }) => {
                assert_eq!(pubkey, "cafebabe");
                assert_eq!(correlation_id.as_deref(), Some("test-cid-unfollow"));
            }
            other => panic!("expected ActorCommand::Unfollow, got {other:?}"),
        }
    }
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
