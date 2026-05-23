//! Chirp's social-verb [`ActionModule`] impls (`nmp.nip25.react`, `nmp.follow`,
//! `nmp.unfollow`) and the per-NIP-crate registration helpers (`register_*_actions`)
//! invoked from [`super::register::nmp_app_chirp_register`].
//!
//! The `ChirpReactModule` / `ChirpFollowModule` / `ChirpUnfollowModule` structs
//! are the D0-clean replacement for the deleted per-verb C symbols
//! (`nmp_app_react`, `nmp_app_follow`, `nmp_app_unfollow`): the social verbs
//! now live in this app crate and reach the kernel through the generic
//! `dispatch_action` path, not through bespoke `nmp-core` FFI symbols.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::{ActorCommand, NmpApp};

use super::helpers::{PubkeyAction, ReactAction};

/// Register Chirp's social-verb action namespaces against `app`'s action
/// registry. Each namespace gets BOTH a module (shape validator, consumed by
/// `ActionRegistry::start`) AND an executor (the `ActorCommand` enqueue,
/// consumed by `ActionRegistry::execute`) — `nmp_app_dispatch_action`
/// requires both halves.
///
/// This is the D0-clean replacement for the deleted per-verb C symbols
/// (`nmp_app_react`, `nmp_app_follow`, `nmp_app_unfollow`): the social verbs
/// now live in this app crate and reach the kernel through the generic
/// `dispatch_action` path, not through bespoke `nmp-core` FFI symbols.
///
/// JSON schemas (the third arg the host passes to `nmp_app_dispatch_action`):
/// * `chirp.react`   — `{"target_event_id":"<hex>","reaction":"+"}`
/// * `nmp.follow`    — `{"pubkey":"<hex>"}`
/// * `nmp.unfollow`  — `{"pubkey":"<hex>"}`
///
/// Hex-shape validation deliberately stays in the actor's command handlers
/// (which own the user-facing toasts) — the module validators here only check
/// JSON shape, mirroring the comment the deleted FFI symbols carried (D6).
pub(super) struct ChirpReactModule;
impl ActionModule for ChirpReactModule {
    const NAMESPACE: &'static str = "nmp.nip25.react";
    type Action = ReactAction;
    fn start(_ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(action: Self::Action, correlation_id: &str, send: &dyn Fn(ActorCommand)) -> Result<(), String> {
        // Thread the registry-minted correlation_id through to the actor so
        // the publish engine reports the terminal verdict under THIS id (not
        // the kind:7 event id); the host spinner keyed on the dispatch
        // return value can then be cleared.
        send(ActorCommand::React {
            target_event_id: action.target_event_id,
            reaction: action.reaction,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

pub(super) struct ChirpFollowModule;
impl ActionModule for ChirpFollowModule {
    const NAMESPACE: &'static str = "nmp.follow";
    type Action = PubkeyAction;
    fn start(_ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(action: Self::Action, correlation_id: &str, send: &dyn Fn(ActorCommand)) -> Result<(), String> {
        // Thread the registry-minted correlation_id through so the kind:3
        // publish terminal verdict reports it; see `ChirpReactModule`.
        send(ActorCommand::Follow {
            pubkey: action.pubkey,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

pub(super) struct ChirpUnfollowModule;
impl ActionModule for ChirpUnfollowModule {
    const NAMESPACE: &'static str = "nmp.unfollow";
    type Action = PubkeyAction;
    fn start(_ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(action: Self::Action, correlation_id: &str, send: &dyn Fn(ActorCommand)) -> Result<(), String> {
        // Thread the registry-minted correlation_id through so the kind:3
        // publish terminal verdict reports it; see `ChirpReactModule`.
        send(ActorCommand::Unfollow {
            pubkey: action.pubkey,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

pub(super) fn register_chirp_actions(app: &mut NmpApp) {
    app.register_action::<ChirpReactModule>();
    app.register_action::<ChirpFollowModule>();
    app.register_action::<ChirpUnfollowModule>();
}

/// Register the 3 NIP-29 group-chat action namespaces against `app`'s action
/// registry.
///
/// This wires typed `ActionModule` impls from the `nmp-nip29` protocol crate
/// via `NmpApp::register_action::<M>()` — the ADR-0027 single-call path that
/// eliminates the pre-ADR-0027 `register_action_module` + `register_action_executor`
/// split. Any NIP crate's typed `ActionModule` can be reached through the
/// generic `dispatch_action` path without `nmp-core` learning any NIP-29
/// group nouns (D0).
///
/// `M::start` handles validation and `M::execute` handles execution — both
/// under the same `M::NAMESPACE`, so namespace mismatch between validator and
/// executor is structurally impossible.
///
/// Namespaces: `nmp.nip29.post_chat_message`, `nmp.nip29.react_in_group`,
/// `nmp.nip29.comment_in_group`, `nmp.nip29.discover`, `nmp.nip29.join`.
///
/// SCOPE: NIP-29 v1 ships chat (3 actions), discovery, and join. The admin /
/// membership (9000-9009) and artifact / discussion executors are deliberately
/// out of scope — Marmot MLS covers private groups; group administration UI
/// is not planned for this milestone.
///
/// `nmp.nip29.discover` is structurally different from the four publish-side
/// actions: it returns [`ActorCommand::PushInterest`] (subscribe to the
/// host relay's kind:39000/39001/39002 catalog), not
/// `PublishUnsignedEventToRelays`. The companion read-side is
/// [`super::register::nmp_app_chirp_register_group_discovery`] — a
/// `DiscoveredGroupsProjection` scoped to the same relay.
pub(super) fn register_nip29_actions(app: &mut NmpApp) {
    nmp_nip29::register_actions(app);
}

/// Register the NIP-17 direct-message `ActionModule` (`nmp.nip17.send`) against
/// `app`'s action registry.
///
/// Wires the typed [`SendDmAction`] from the `nmp-nip17` protocol crate
/// through the same host-extensibility seam the NIP-29 actions use. The
/// executor delegates to `nmp_nip17::SendDmAction::execute`, which builds the
/// kind:14 rumor and enqueues [`ActorCommand::SendGiftWrappedDm`] — the
/// actor's local-keys-MVP handler does the NIP-59 seal + gift-wrap + publish.
///
/// JSON schema (the third arg the host passes to `nmp_app_dispatch_action`):
/// * `nmp.nip17.send` — `{"recipient_pubkey":"<hex>","content":"…","reply_to":"<hex>"?}`
/// * `nmp.nip17.publish_relay_list` — `{"relays":["wss://relay.example", ...]}`
///
/// `nmp.nip17.publish_relay_list` closes the symmetric publish gap: the kernel
/// ingests kind:10050 (NIP-17 DM-relay list) into `dm_relay_lists`, but
/// without a publish path every NMP user is invisible to other clients
/// trying to send them gift-wrapped DMs. The executor builds the kind:10050
/// unsigned event with `["relay", <url>]` tags and enqueues
/// `ActorCommand::PublishUnsignedEvent` — kind:10050 is a NIP-65 replaceable
/// event and routes through the author's kind:10002 write relays.
pub(super) fn register_nip17_actions(app: &mut NmpApp) {
    nmp_nip17::register_actions(app);
}

/// Register the NIP-57 lightning-zap [`ActionModule`] (`nmp.nip57.zap`)
/// against `app`'s action registry.
///
/// Wires the typed [`ZapAction`] from the `nmp-nip57` protocol crate
/// through the same host-extensibility seam the NIP-17 / NIP-29 actions
/// use. The executor builds the unsigned kind:9734 zap request via
/// [`nmp_nip57::ZapRequestBuilder`] and enqueues
/// [`nmp_core::ActorCommand::FetchLnurlInvoice`] — the actor signs the
/// kind:9734 on-thread (D7), then spawns a worker thread for the
/// LNURL-pay HTTP round-trip (D8 — no blocking on the actor thread).
///
/// JSON schema (the third arg the host passes to
/// `nmp_app_dispatch_action`):
///
/// ```json
/// {
///   "recipient_pubkey": "<hex>",
///   "amount_msats": 21000,
///   "lnurl": "alice@walletofsatoshi.com",
///   "relays": ["wss://relay.damus.io"],
///   "target_event_id": "<hex>",   // optional
///   "comment": "🤙"              // optional
/// }
/// ```
///
/// `lnurl` accepts any of the three LNURL-pay input shapes: a
/// lightning address (`user@domain` per LUD-16), a bech32 LNURL
/// (`lnurl1…` per LUD-01), or a bare `https://` URL.
///
/// # Observable surface
///
/// The actor's `FetchLnurlInvoice` handler surfaces results through
/// two channels:
///
/// 1. [`ActorCommand::ShowToast`] — the bolt11 invoice on success
///    (`Zap invoice: lnbc…`) or a human-readable reason on failure
///    (`Zap failed: …`). This is the ADR-0024 minimum-viable
///    observable; a `last_action_outcomes` snapshot projection is the
///    designed follow-up.
/// 2. The `action_stages` mirror — `Requested` is set when the
///    dispatch arm fires; `Failed { reason }` is recorded on any
///    pre-payment failure so a host spinner keyed on the
///    `dispatch_action` correlation_id clears on the next tick.
///
/// # Out-of-scope
///
/// * **NWC payment**. The handler returns the bolt11 invoice but does
///   not pay it; the wallet handoff
///   ([`ActorCommand::WalletPayInvoice`], gated by the `wallet` feature)
///   is the next milestone.
/// * **Bunker (NIP-46) signing of kind:9734**. The actor reads
///   `IdentityRuntime::active_local_keys`; bunker accounts fail closed
///   with a clear toast (ADR-0026 Phase 2 follow-up, parallel to the
///   NIP-17 DM bunker-send path).
pub(super) fn register_nip57_actions(app: &mut NmpApp) {
    nmp_nip57::register_actions(app);
}

/// Register the NIP-65 relay-list `ActionModule` (`nmp.nip65.publish_relay_list`)
/// against `app`'s action registry.
///
/// Wires the typed [`PublishRelayListAction`] from the `nmp-nip65` protocol
/// crate through the same host-extensibility seam the NIP-17 / NIP-29 / NIP-57
/// actions use. The executor builds the kind:10002 unsigned event with
/// `["r", <url>]` / `["r", <url>, "read"]` / `["r", <url>, "write"]` tags and
/// enqueues [`ActorCommand::PublishUnsignedEvent`] — kind:10002 is a NIP-65
/// replaceable event and routes through the kernel's Auto path so the very
/// first kind:10002 for a freshly-created account hits the bootstrap
/// discovery relays (no chicken-and-egg) and later updates land on the
/// author's own write set.
///
/// JSON schema (the third arg the host passes to `nmp_app_dispatch_action`):
///
/// ```json
/// {
///   "relays": [
///     { "url": "wss://relay.example" },                         // both
///     { "url": "wss://outbox.example", "marker": "write" },     // write-only
///     { "url": "wss://inbox.example",  "marker": "read"  }      // read-only
///   ]
/// }
/// ```
///
/// # Why register this alongside the AddRelay/RemoveRelay auto-trigger?
///
/// `actor::dispatch` already piggybacks a kind:10002 re-publish onto every
/// `AddRelay` / `RemoveRelay` mutation (see `maybe_publish_relay_list_after_edit`
/// in `crates/nmp-core/src/actor/dispatch.rs`). The dispatched action seam
/// here is the host-facing twin: a host that wants to advertise a relay set
/// it derived in app land (e.g. on first login, before any AddRelay edits)
/// can call `nmp_app_dispatch_action("nmp.nip65.publish_relay_list", json)`
/// and get a `correlation_id` + lifecycle entries it can spinner on. Both
/// paths converge on the same on-wire kind:10002 — the auto-trigger reads
/// `RelayEditRow`, the action takes explicit input.
pub(super) fn register_nip65_actions(app: &mut NmpApp) {
    nmp_nip65::register_actions(app);
}
