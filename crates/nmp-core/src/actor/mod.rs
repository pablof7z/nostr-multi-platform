//! Actor main loop — message routing, command dispatch, relay event handling.
//!
//! Idle-tick timing helpers are in `tick.rs`.
//! Relay lifecycle helpers are in `relay_mgmt.rs`.
//!
//! # Dual-channel priority design
//!
//! Commands (`command_rx`) are checked via `try_recv` at the top of every
//! iteration with a bounded burst budget — low latency, never dropped under
//! relay event flood, while relay events and idle work still progress during
//! sustained command bursts.
//! Relay events go through their own separate channel, read via
//! `recv_timeout(compute_wait(…))`. This replaces the old merged
//! `SyncSender<ActorMsg>` design where a 4096-slot bounded channel could fill
//! with relay events and cause `try_send` to silently drop commands like
//! `CreateAccount` during onboarding.

mod commands;
mod dispatch;
mod fairness;
pub(crate) mod kernel_action;
mod outbound;
mod pending_sign;
#[cfg(test)]
mod publish_relay_dispatch_tests;
mod relay_mgmt;
mod relay_roles;
#[cfg(test)]
mod relay_url_canonical_tests;
mod session_persistence;
#[cfg(test)]
mod session_persistence_tests;
#[cfg(test)]
mod tests;
mod tick;

use crate::capability_socket::{new_capability_callback_slot, CapabilityCallbackSlot};
use commands::IdentityRuntime;
// D0: NIP-47 NWC is an app noun — `WalletRuntime` only exists with `wallet`.
#[cfg(feature = "wallet")]
use commands::WalletRuntime;
// D0: NIP-47 NWC is an app noun — the wallet-status slot is re-exported so the
// `ffi` module can build it, hand one clone to the actor, and capture the
// other in the `"wallet"` snapshot-projection closure.
#[cfg(feature = "wallet")]
pub(crate) use commands::{new_wallet_status_slot, WalletStatusSlot};
// `WalletStatus` itself only crosses the module boundary for the
// snapshot-projection test, which constructs a status value to drive the
// `"wallet"` projection through `make_update`.
#[cfg(all(test, feature = "wallet"))]
pub(crate) use commands::WalletStatus;
pub(crate) use commands::{
    new_event_observer_slot, new_observer_slot as new_lifecycle_observer_slot, notify_observers,
    register_c_observer, register_rust_observer, unregister_observer, KernelEventObserverSlot,
    LifecycleObserverRegistration, LifecycleObserverSlot,
};
// D0: NIP-46 remote signing is an app noun — the bunker-handshake slot is
// re-exported so the `ffi` module can build it, hand one clone to the actor's
// `IdentityRuntime`, and capture the other in the built-in
// `"bunker_handshake"` snapshot-projection closure.
pub(crate) use commands::{
    build_nip46_onboarding_dto, new_bunker_handshake_slot, BunkerHandshakeSlot,
};
// `pub` (not `pub(crate)`) so the `lib.rs` test-support re-export reaches
// integration tests outside the crate. The `actor` module itself is
// crate-private (`mod actor;` in `lib.rs`), so external Rust callers still
// see these only via the gated `pub use actor::{...}` in lib.rs. The
// constants are unused inside the crate (FFI consumers read them through
// the test-support facade), so allow-unused keeps a plain `cargo build`
// clean.
#[allow(unused_imports)]
pub use commands::{LifecycleObserverFn, LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};
// T146 — re-export the kernel event observer types so external Rust callers
// (per-app crates such as `nmp-app-chirp`) can implement and register
// `KernelEventObserver`s through the gated `pub use actor::{...}` in
// `lib.rs`. The FFI shape (`KernelEventObserverFn` /
// `KernelEventObserverRegistration` / `KernelEventObserverId`) is also
// surfaced so Swift / Kotlin bindings can use the C-ABI channel.
#[allow(unused_imports)]
pub use commands::{
    KernelEventObserver, KernelEventObserverFn, KernelEventObserverId,
    KernelEventObserverRegistration,
};
// Raw signed-event tap — re-export the slot helpers (crate-private) so
// `ffi/raw_event_tap.rs` and the actor entry point reach the shared slot,
// and the public wire shapes so per-app Rust crates + Swift / Kotlin
// bindings can register a verbatim signed-event observer.
#[allow(unused_imports)]
pub(crate) use commands::{
    new_raw_event_observer_slot, notify_raw_observers, raw_observers_idle_for_kind,
    register_c_raw_observer, register_rust_raw_observer, unregister_raw_observer,
    RawEventObserverSlot,
};
#[allow(unused_imports)]
pub use commands::{
    KindFilter, RawEventObserver, RawEventObserverFn, RawEventObserverId,
    RawEventObserverRegistration,
};
// NIP golden-tag conformance harness — re-exported up the (crate-private)
// `actor` chain so the gated `pub use actor::ConformanceHarness` in `lib.rs`
// reaches the `tests/nip_tag_conformance.rs` integration test. Gated on
// `test-support` so it never appears in a production build.
#[cfg(any(test, feature = "test-support"))]
pub use commands::ConformanceHarness;
use dispatch::{dispatch_command, handle_relay_event, ActorContext};
use fairness::{CommandDrain, COMMAND_DRAIN_BUDGET};
use pending_sign::PendingSign;

use crate::kernel::LifecyclePhase;

use crate::app::KernelAction;

use relay_mgmt::{
    all_relays_connected, close_relays, maybe_send_startup, route_dispatch_outbound,
    send_all_outbound,
};
use tick::{compute_wait, emit_now, flush_due};

use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, RelayRole, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use crate::relay_worker::{RelayCommand, RelayEvent};
use std::collections::{HashMap, HashSet};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use relay_roles::NOSTRCONNECT_DEFAULT_RELAY_URL;
pub(crate) use relay_roles::{
    canonical_relay_role, has_role, nostrconnect_relay_url, relay_role_label, relay_role_options,
    relay_role_tint,
};

/// Actor command variants.  The `actor` module is private (`mod actor`, not
/// `pub mod actor`), so this `pub` is only reachable from outside the crate
/// through the `testing` re-export gate.  In normal (non-test-support) builds
/// nothing re-exports these items, so they remain effectively crate-private.
#[derive(Debug)]
pub enum ActorCommand {
    Start {
        visible_limit: usize,
        emit_hz: u32,
    },
    Configure {
        visible_limit: usize,
        emit_hz: u32,
    },
    OpenAuthor {
        pubkey: String,
    },
    OpenThread {
        event_id: String,
    },
    OpenFirehoseTag {
        tag: String,
    },
    /// T66a identity — import an nsec/hex secret, add to the actor-local
    /// identity store, bind it as the active signer, retarget the timeline.
    ///
    /// The `secret` is carried as [`zeroize::Zeroizing<String>`] so the
    /// plaintext nsec is wiped from memory the instant the command is dropped
    /// — the in-flight window between FFI ingest and key parsing is minimized.
    SignInNsec {
        secret: zeroize::Zeroizing<String>,
    },
    /// T66a identity — parse a `bunker://` NIP-46 URI. Transport is NOT yet
    /// wired (D0 forbids `nmp-core -> nmp-signers`); this validates the URI
    /// shape and surfaces a `last_error_toast` directing the user to nsec.
    SignInBunker {
        uri: String,
    },
    /// Create a new keypair, publish a kind:0 metadata event and a kind:10002
    /// relay-list event, then register the identity and make it active.
    ///
    /// `profile` is a map of key/value pairs that is JSON-serialised into the
    /// kind:0 `content` field.  `relays` is a list of `(url, role)` tuples
    /// where `role` is `"read"`, `"write"`, `"both"`, `"indexer"`, or a
    /// comma-separated composite such as `"both,indexer"`. `mls` requests
    /// account-scoped MLS setup in app composition crates.
    CreateAccount {
        profile: HashMap<String, String>,
        relays: Vec<(String, String)>,
        mls: bool,
    },
    /// T66a identity — switch the active account (synchronous re-bind +
    /// timeline retarget, mirrors AccountManager::switch_active semantics).
    SwitchActive {
        identity_id: String,
    },
    /// T66a identity — remove an account; clears the active slot if it was
    /// the active one.
    RemoveAccount {
        identity_id: String,
    },
    /// Broker → actor: register a fully-handshaken remote signer (e.g.
    /// completed NIP-46 bunker handshake). Actor inserts into
    /// `IdentityRuntime.remote_signers` and emits a snapshot update.
    /// Becomes active if no account was active. D0 stays clean — the
    /// trait object's concrete type lives in `nmp-signers` but `nmp-core`
    /// only sees `dyn RemoteSignerHandle` (defined in
    /// [`crate::remote_signer`]).
    ///
    /// Constructed by the `nmp-signer-broker` crate, which depends on both
    /// `nmp-core` and `nmp-signers`. It has a live production caller
    /// (`BunkerBroker::spawn_handshake` in `nmp-signer-broker/src/broker.rs`);
    /// `#[allow(dead_code)]` only suppresses rustc's *per-crate* dead-code
    /// lint, which cannot see the cross-crate constructor.
    #[allow(dead_code)]
    // live cross-crate caller in nmp-signer-broker — per-crate lint false positive
    AddRemoteSigner {
        handle: Box<dyn crate::RemoteSignerHandle>,
    },
    /// Broker → actor: progress event for the bunker handshake UI. Actor
    /// stores the latest into a kernel snapshot field; the broker is the
    /// sole writer. Stage `"idle"` clears the projection. Has a live
    /// production caller (`BunkerBroker::emit_progress` in
    /// `nmp-signer-broker/src/broker.rs`); `#[allow(dead_code)]` only
    /// suppresses rustc's per-crate lint, which cannot see it.
    #[allow(dead_code)]
    // live cross-crate caller in nmp-signer-broker — per-crate lint false positive
    BunkerHandshakeProgress {
        /// `"connecting"` | `"awaiting_pubkey"` | `"ready"` | `"failed"` | `"idle"`.
        stage: String,
        /// Optional human-readable status (e.g. relay URL, error reason).
        message: Option<String>,
    },
    /// T66a publish — sign a kind:1 (optionally a reply) with the active
    /// account and emit it to the NIP-65 outbox-resolved write relays (D3).
    ///
    /// `correlation_id` is the registry-minted action id when this command
    /// originates from `nmp_app_dispatch_action` (`PublishAction::PublishNote`).
    /// The actor signs the event, so its `id` is unknown at dispatch time and
    /// `preferred_action_id()` could not pre-bind the host's correlation_id to
    /// it. Threading the minted id here makes the publish engine report it in
    /// `action_results` (instead of the signed event's `id`), so the host
    /// spinner keyed on the dispatch return value can be cleared. `None` for
    /// the legacy non-dispatch callers — the engine then falls back to the
    /// publish handle (== event id), preserving the prior behaviour.
    PublishNote {
        content: String,
        reply_to_id: Option<String>,
        target: crate::publish::PublishTarget,
        correlation_id: Option<String>,
    },
    /// Sign-and-publish an arbitrary event kind for the active account.
    /// The actor fills `pubkey` from the active signer, stamps `created_at`
    /// (D7), signs, and routes through the NIP-65 outbox per `target`.
    /// Dispatched by `PublishAction::PublishRaw` via `dispatch_action`.
    ///
    /// Both local-keys and remote (NIP-46) signer accounts are supported —
    /// the dispatch arm delegates to the existing `publish_unsigned_event` /
    /// `publish_unsigned_event_to_relays` helpers, which already park bunker
    /// signs in `PendingSign` (D8 — actor never blocks).
    PublishRawEvent {
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        target: crate::publish::PublishTarget,
        correlation_id: Option<String>,
    },
    /// T66a publish — sign a kind:0 profile metadata event with the active
    /// account and emit it to the NIP-65 outbox-resolved write relays (D3).
    ///
    /// `fields` is the flat string map the host supplied; the actor serializes
    /// it into the kind:0 `content`, stamps `created_at` from `kernel.now_secs()`
    /// (the host never hand-rolls the timestamp), and signs. Sibling of
    /// [`ActorCommand::PublishNote`] — same sign-and-publish path, kind:0 instead
    /// of kind:1.
    ///
    /// `correlation_id` is the registry-minted action id when this command
    /// originates from `nmp_app_dispatch_action` (`PublishAction::PublishProfile`).
    /// Threading it through makes the publish engine report it in
    /// `action_results` so the host spinner keyed on the dispatch return
    /// value can be cleared. `None` for non-dispatch callers.
    PublishProfile {
        fields: serde_json::Map<String, serde_json::Value>,
        correlation_id: Option<String>,
    },
    /// Generic, kind-agnostic publish — take an `UnsignedEvent` already built
    /// by any protocol-crate builder (`nmp_nip23::Article`, `nmp_nip01::Note`,
    /// `nmp_relations::Reaction`, …), sign with the active account's keys,
    /// and route through the NIP-65 outbox resolver (D3). The kernel does
    /// not inspect the kind — that's the protocol crate's concern (D0).
    ///
    /// Stepping stone toward per-protocol-crate `ActionModule` impls
    /// (`kind-wrappers.md` §8 Phase 1); deprecates kind-by-kind as those land.
    ///
    /// `correlation_id` is the registry-minted action id when this command
    /// originates from an `ActionModule::execute` call. Threading it lets the
    /// publish engine report THAT id in `action_results` (via
    /// `correlation_id_override`) so the host spinner closes on the id it
    /// received from `dispatch_action`, not on the signed event's id.
    /// `None` for callers that are not action-dispatched (e.g. direct
    /// `NmpApp::` Rust API calls, conformance tests).
    PublishUnsignedEvent {
        event: crate::substrate::UnsignedEvent,
        correlation_id: Option<String>,
    },
    /// Publish an unsigned event to an explicit relay set, bypassing the
    /// NIP-65 outbox resolver. Used by action executors that target a
    /// specific relay pin (e.g. NIP-29 group relays). D4: only the actor
    /// signs and publishes. D8: no blocking — relay dispatch is async.
    ///
    /// Sibling to [`ActorCommand::PublishUnsignedEvent`] (which routes via the
    /// NIP-65 outbox) and [`ActorCommand::PublishSignedEvent`] (which carries
    /// an already-signed event). This variant SIGNS with the active account
    /// like the unsigned sibling, but ROUTES to exactly `relays` like the
    /// signed sibling's `Explicit` mode — the combination a host-pinned group
    /// action needs. A NIP-29 join request must reach the group's own host
    /// relay, never the author's kind:10002 outbox.
    ///
    /// Like the unsigned sibling, the event's `pubkey` is derived from the
    /// active identity at sign time; the caller's `event.pubkey` is ignored.
    /// Empty or malformed `relays` fail closed in the publish handler. Callers
    /// that want NIP-65 outbox routing must use [`ActorCommand::PublishUnsignedEvent`]
    /// so `Auto` and `Explicit` never share the same empty-vector encoding.
    PublishUnsignedEventToRelays {
        event: crate::substrate::UnsignedEvent,
        relays: Vec<crate::publish::RelayUrl>,
        /// Registry-minted `correlation_id` from `dispatch_action`, when this
        /// command originates from an `ActionModule::execute` call. Threading
        /// it lets the publish engine report THAT id in `action_results`
        /// (via `correlation_id_override`) so the host spinner closes on the
        /// id it received from `dispatch_action`, not on the signed event's id.
        /// `None` for callers that are not action-dispatched (e.g. direct
        /// `NmpApp::` Rust API calls).
        correlation_id: Option<String>,
    },
    /// Generic publish of an **already-signed** event. The kernel verifies
    /// the Schnorr signature + event-id hash, then routes the event verbatim
    /// through the same planner / NIP-65 outbox / relay-pin path the unsigned
    /// command uses — the signer is never consulted (no re-signing). Unlike
    /// [`ActorCommand::PublishUnsignedEvent`], this does not require an active
    /// account: the signature already exists and routing keys off the event's
    /// own pubkey. Generic capability (D0); externally-signed group events are
    /// the first consumer but the kernel has no protocol nouns.
    ///
    /// `target` selects the D3 routing mode without erasing intent:
    /// `Auto` asks the kernel to resolve via NIP-65, while
    /// `Explicit { relays }` dispatches to exactly those relays and fails
    /// closed when the set is empty or malformed.
    ///
    /// `correlation_id` is the registry-minted action id when this publish
    /// originates from `nmp_app_dispatch_action`'s `PublishAction::Publish`
    /// path. Threading it makes the publish engine report THAT id in
    /// `action_results` (via `correlation_id_override`) — explicit symmetry
    /// with the `PublishNote` path. `None` for non-dispatch callers
    /// (`NmpApp::publish_signed_explicit` — Marmot's MLS / gift-wrap seam,
    /// which replaced the deleted `nmp_app_publish_signed_event*` symbols
    /// with this typed Rust API — and conformance harnesses); the engine
    /// then falls back to the publish handle (== event id), preserving
    /// prior behaviour. The pre-signed `Publish` round-trip already happened
    /// to work by coincidence (`preferred_action_id` returns `event.id`, the
    /// `None`-fallback also reports `event.id`); this field upgrades that
    /// coincidence into an explicit guarantee a host can rely on.
    PublishSignedEvent {
        raw: crate::store::RawEvent,
        target: crate::publish::PublishTarget,
        correlation_id: Option<String>,
    },
    /// Send a NIP-17 gift-wrapped DM. The actor constructs one kind:1059
    /// envelope per recipient and one self-copy, using the active signer's
    /// keys. Each envelope is published to *its receiver's* kind:10050 DM-inbox
    /// relays (NIP-17 § 2) — the recipient envelope to the recipient's list,
    /// the self-copy to the sender's own list. If either receiver's kind:10050
    /// list is missing or empty, the handler fails closed with a toast and
    /// emits no publish frames; kind:1059 must not fall back to generic Content
    /// relays.
    ///
    /// `rumor` is the **unsigned** kind:14 chat-message event built by
    /// `nmp_nip17::build_dm_rumor` — it is never signed or published as-is.
    /// The actor seals + gift-wraps it (NIP-59 `gift_wrap`) into kind:1059
    /// envelopes whose outer signatures use fresh per-envelope ephemeral keys
    /// (the unlinkability guarantee). Two envelopes are produced: one wrapped
    /// to `recipient_pubkey`, one wrapped to the sender's own pubkey (the
    /// self-copy, so sent messages remain readable).
    ///
    /// The gift-wrap crypto runs on the actor thread (D7 — the kernel owns key
    /// access and the wall clock). The `rumor.created_at` is re-stamped from
    /// `kernel.now_secs()` before wrapping.
    ///
    /// # Phase 1 — local keys only
    ///
    /// `nmp_nip59::gift_wrap` requires `&nostr::Keys` because it thin-wraps
    /// `nostr::EventBuilder::gift_wrap(&Keys, ...)` — raw keys end-to-end. A
    /// remote (NIP-46) signer exposes no local key, so bunker accounts cannot
    /// use this path; the actor detects the missing key and surfaces a toast
    /// (D6 — explicit failure, never silent, never a panic). Bunker support
    /// requires a new `nmp_nip59::gift_wrap_with_signer` that calls
    /// `nostr::nips::nip59::make_seal(signer, receiver, rumor)` for the
    /// kind:13 seal step (NIP-44 via `RemoteSignerHandle::nip44_encrypt`,
    /// ADR-0026) and mints an ephemeral key locally for the kind:1059 wrap.
    SendGiftWrappedDm {
        rumor: crate::substrate::UnsignedEvent,
        recipient_pubkey: String,
        /// Registry-minted action id when this send originates from
        /// `nmp_app_dispatch_action` (`nmp.nip17.send`). The actor records
        /// `ActionStage::Requested` against this id and the per-envelope
        /// `publish_signed_event` calls thread it through to the publish
        /// engine's `correlation_id_override`, so the kind:1059 terminal
        /// verdict (or any pre-publish early-exit failure) lands in
        /// `action_results` and the host spinner resolves. Non-dispatch
        /// callers (conformance harnesses) pass `None`.
        correlation_id: Option<String>,
    },
    /// User intent from the outbox UI: retry a still-pending publish now.
    RetryPublish {
        handle: String,
    },
    /// User intent from the outbox UI: cancel a still-pending publish.
    CancelPublish {
        handle: String,
    },
    /// T66a publish — kind:7 reaction to `target_event_id`.
    React {
        target_event_id: String,
        reaction: String,
        /// Registry-minted action id when this React originates from
        /// `nmp_app_dispatch_action` (`chirp.react`). The publish engine
        /// reports the verdict under this id (via
        /// `publish_signed_with_correlation`) so the host spinner keyed on
        /// the dispatch return value can be cleared. Sign-step early exits
        /// also use it to record a `Failed` terminal via
        /// `record_action_failure`. Non-dispatch callers pass `None`.
        correlation_id: Option<String>,
    },
    /// T66a publish — append `pubkey` to the active account's kind:3 follow
    /// set and re-publish it.
    Follow {
        pubkey: String,
        /// Registry-minted action id when this Follow originates from
        /// `nmp_app_dispatch_action` (`chirp.follow`). See `React` for the
        /// spinner round-trip contract.
        correlation_id: Option<String>,
    },
    /// T66a publish — remove `pubkey` from the kind:3 follow set.
    Unfollow {
        pubkey: String,
        /// Registry-minted action id when this Unfollow originates from
        /// `nmp_app_dispatch_action` (`chirp.unfollow`). See `React` for the
        /// spinner round-trip contract.
        correlation_id: Option<String>,
    },
    /// T66a relay edit — add a relay row (role: `read` | `write` | `both`).
    AddRelay {
        url: String,
        role: String,
    },
    /// T66a relay edit — remove a relay row.
    RemoveRelay {
        url: String,
    },
    /// T66a — (re)open the following-timeline for the active account.
    OpenTimeline,
    ClaimProfile {
        pubkey: String,
        consumer_id: String,
    },
    ReleaseProfile {
        pubkey: String,
        consumer_id: String,
    },
    CloseAuthor {
        pubkey: String,
    },
    CloseThread {
        event_id: String,
    },
    /// NIP-47 wallet connect — parse the `nostr+walletconnect://` URI, subscribe
    /// for kind:23195 responses, and send get_info + get_balance requests.
    /// D0: gated behind the `wallet` feature — NIP-47 NWC is an app noun.
    #[cfg(feature = "wallet")]
    WalletConnect {
        uri: String,
    },
    /// NIP-47 wallet disconnect — close the subscription and clear state.
    /// D0: gated behind the `wallet` feature — NIP-47 NWC is an app noun.
    #[cfg(feature = "wallet")]
    WalletDisconnect,
    /// NIP-47 pay invoice — sign and send a `pay_invoice` kind:23194 request.
    /// D0: gated behind the `wallet` feature — NIP-47 NWC is an app noun.
    ///
    /// `correlation_id` is the registry-minted action id when this command
    /// originates from `nmp_app_dispatch_action` (a future `nmp.zap`
    /// ActionModule executor — the C-ABI `nmp_app_wallet_pay_invoice` symbol
    /// that the iOS shell calls today passes `None`). The wallet runtime
    /// stores `event_id → correlation_id` in its per-connection
    /// `pending_payments` map when the kind:23194 request is built, then
    /// drains it in `handle_nwc_text` on the matching kind:23195 response
    /// and routes the outcome to [`Kernel::record_action_success`] (preimage
    /// returned) or [`Kernel::record_action_failure`] (`error` object) so
    /// the host spinner keyed on the dispatch return value can be cleared.
    /// `None` is a no-op on the response side — nothing is waiting on an id.
    #[cfg(feature = "wallet")]
    WalletPayInvoice {
        bolt11: String,
        amount_msats: Option<u64>,
        correlation_id: Option<String>,
    },
    /// NIP-57 LNURL-pay round-trip. The actor signs `unsigned` (the kind:9734
    /// zap request) with the active local identity, then spawns a worker
    /// thread that completes the two-leg LNURL-pay HTTP round-trip
    /// (well-known fetch → callback fetch) and surfaces the resulting bolt11
    /// invoice as a [`ActorCommand::ShowToast`] follow-up.
    ///
    /// `lnurl_or_address` may be a lightning address (`user@domain`), a
    /// bech32 `lnurl1…`, or a bare `https://` URL — `commands::zap` decodes
    /// all three shapes into the LNURL-pay well-known URL per LUD-01/06/16.
    ///
    /// # NIP-57 wire-routing — kind:9734 NEVER reaches relays
    ///
    /// The signed zap request is delivered to the LNURL callback as a
    /// `nostr=<urlencoded>` query parameter (NIP-57 § "Appendix C"). It is
    /// NOT broadcast to Nostr relays — the receipt (kind:9735) is, and the
    /// LN provider mints it after the invoice settles. This arm therefore
    /// emits NO `PublishUnsignedEventToRelays` follow-up; any caller that
    /// expects relay traffic from a zap intent has misunderstood NIP-57.
    ///
    /// # ADR-0026 Phase 1 — local keys only
    ///
    /// Bunker (NIP-46 remote-signer) accounts fail closed with a clear
    /// toast and a `RecordActionFailure` (when a `correlation_id` was
    /// supplied) — kind:9734 signing through a remote signer is the
    /// follow-up parallel to the NIP-17 DM Phase-2 work.
    ///
    /// # ADR-0024 minimum-viable observable surface
    ///
    /// The bolt11 invoice is surfaced via `ShowToast`. A snapshot-projection
    /// surface (`last_action_outcomes` per memory note #57) is the designed
    /// follow-up; the toast is the minimum-viable observable so a host can
    /// substring-match the `lnbc…` prefix and drive its NWC pay flow.
    /// `correlation_id` is the registry-minted action id when this arm
    /// originates from `dispatch_action` (`nmp.nip57.zap`); a `Failed`
    /// terminal is recorded against it on any pre-payment failure so the
    /// host spinner clears.
    FetchLnurlInvoice {
        unsigned: crate::substrate::UnsignedEvent,
        lnurl_or_address: String,
        amount_msats: u64,
        correlation_id: Option<String>,
    },
    /// T118 / G3 — app lifecycle phase transition reported by the host shell
    /// (or any conforming consumer). The actor folds the phase into the
    /// kernel's [`crate::kernel::LifecyclePhase`] state and, on a
    /// meaningful transition (`Background → Foreground`, `Foreground →
    /// Background`, or first phase after boot), fires the registered
    /// lifecycle observer. The observer is what fans the transition out to
    /// the shell's sync-trigger engine (typically on a foreground
    /// transition); nmp-core itself does not name any shell vocabulary (D0).
    /// Idempotent: rapid scene oscillation debounces to a single observer
    /// call per transition.
    LifecycleEvent(LifecyclePhase),
    /// Host acknowledgement of a `correlation_id` in the
    /// `action_stages` snapshot mirror. The actor folds the ack into the
    /// kernel's `ActionStageTracker`, dropping the entry's stage history
    /// so the next tick's snapshot no longer carries it. Idempotent: an
    /// unknown id is a silent no-op (D6).
    ///
    /// Originates from the FFI symbol `nmp_app_ack_action_stage`. The host
    /// calls this after rendering a terminal stage (`Accepted` or
    /// `Failed`) and clearing its UI; until the ack arrives the entry
    /// stays in the snapshot, so a tick the host missed cannot strand
    /// the action's state machine.
    AckActionStage(String),
    /// Record a terminal `Failed` stage for `correlation_id` on
    /// behalf of an executor that panicked (or otherwise failed *after*
    /// the registry minted the correlation id and before any
    /// `ActorCommand` carrying it could be enqueued).
    ///
    /// Without this seam the failure is orphaned: the host received a
    /// correlation_id from `nmp_app_dispatch_action`'s error envelope but
    /// has no way to ACK an `action_stages` entry that was never produced.
    /// The actor folds this command into [`Kernel::record_action_failure`]
    /// — same engine the sign-step failure path uses — so a `Failed`
    /// terminal lands in both `action_stages` (the mirror, for the host's
    /// ACK lifecycle) and `action_results` (the drain, for the host's
    /// spinner cleanup).
    ///
    /// Originates from [`crate::ffi::action::dispatch_action_json`] on the
    /// FFI thread when the executor returned an `Err` (including a
    /// `catch_unwind`-converted panic). Idempotent w.r.t. a buggy host
    /// that re-sends — `record_action_failure` records a second `Failed`
    /// stage, which is a benign no-op for the host (it sees the same
    /// terminal twice; the second ACK is a silent no-op).
    RecordActionFailure {
        correlation_id: String,
        reason: String,
    },
    /// Record a terminal `Accepted` stage for `correlation_id` on
    /// behalf of an off-thread worker whose success outcome is observed
    /// outside the publish engine. The symmetric counterpart to
    /// [`ActorCommand::RecordActionFailure`]: same routing through
    /// [`Kernel::record_action_success`], which writes both the
    /// `action_stages` mirror (so the host's stage observer sees the
    /// terminal) and the `action_results` per-tick drain (so a spinner
    /// keyed on the `correlation_id` clears).
    ///
    /// The motivating consumer is the NIP-57 zap LNURL-pay worker
    /// (`actor/commands/zap.rs`): after the HTTP round-trip returns a
    /// bolt11 invoice, the spawned worker has no `&mut Kernel` reference
    /// and must round-trip through the actor channel to record the
    /// terminal. Without this command the `dispatch_action`
    /// (`nmp.nip57.zap`) spinner hangs forever — `ShowToast` is a
    /// human-readable surface, NOT the spinner-closing one
    /// (`action_results` is the closing surface).
    ///
    /// Idempotent w.r.t. a buggy worker that re-sends — `record_action_success`
    /// records a second `Accepted` stage, which is a benign no-op for the
    /// host (it sees the same terminal twice; the second ACK is a silent
    /// no-op).
    RecordActionSuccess {
        correlation_id: String,
    },
    Stop,
    Reset,
    Shutdown,
    /// Generic FFI-boundary action (T95). Routed through the
    /// [`dispatch_kernel_action`] reducer; the resolved [`KernelUpdate`] is
    /// serialized and pushed on the update channel. `OpenUri` registers the
    /// resolved interest through the single-writer registry (D4).
    Kernel(KernelAction),
    /// Ingest pre-verified timeline events through the test-support kernel path.
    ///
    /// The caller is responsible for constructing `VerifiedEvent` values; this
    /// command routes each through `kernel::ingest_pre_verified_event` under the
    /// `"diag-firehose-stress"` sub-id. It inserts through the `EventStore`, then
    /// updates the lightweight read-cache directly. No signature re-verification
    /// is performed — the `VerifiedEvent` type is the gate.
    ///
    /// Test-support only (D0: not part of production FFI surface).
    #[cfg(any(test, feature = "test-support"))]
    IngestPreVerifiedEvents(Vec<crate::store::VerifiedEvent>),
    /// D6 — surface an error toast from the FFI boundary. Used when the FFI
    /// layer detects a malformed argument (e.g. unparseable JSON) and cannot
    /// call `kernel.set_last_error_toast` directly (the FFI only has a channel
    /// sender, not a kernel reference). The actor thread receives this command
    /// and routes it to `kernel.set_last_error_toast` so the error becomes
    /// observable state, never a silent no-op.
    ShowToast {
        message: String,
    },
    /// Register a `LogicalInterest` into the subscription registry and trigger
    /// a recompile. Idempotent: same `InterestId` replaces the previous entry.
    ///
    /// Used by protocol crates (e.g. `nmp-marmot`) to register persistent
    /// relay subscriptions (e.g. kind:1059 `#p <pubkey>`) that should remain
    /// live for the session without Swift/Kotlin involvement (D0). The kernel
    /// will emit the appropriate `REQ` frames to connected relays on the next
    /// compile pass; matching inbound events then flow through the raw-event
    /// tap into the host-app service automatically (D4 / event-driven delivery).
    PushInterest(crate::planner::LogicalInterest),
    /// Withdraw a previously registered logical interest by id and trigger a
    /// recompile. Generic lifecycle counterpart to [`PushInterest`].
    WithdrawInterest(crate::planner::InterestId),
}

/// One per-URL relay-worker handle. T105: `relay_url` (NOT `role`) is the
/// pool key — every resolved write/read relay gets its own socket. `role`
/// is retained so the actor can route diagnostic-bucket updates back to
/// the kernel's lane-keyed RelayHealth rows until per-URL health lands (M11).
pub(super) struct RelayControl {
    pub(super) generation: u64,
    #[allow(dead_code)] // Diagnostic lane label; per-URL health is M11.
    pub(super) role: RelayRole,
    #[allow(dead_code)] // The URL this worker dials — the routing key in the pool.
    pub(super) relay_url: String,
    pub(super) tx: Sender<RelayCommand>,
}

use outbound::wire_frames_to_outbound;

/// Backwards-compatible entry point: spawn the actor without a lifecycle
/// observer. Existing tests and the `nmp-core::testing` facade call this
/// shape. The FFI surface uses [`run_actor_with_observers`] instead so the
/// shell can register a phase-transition callback + kernel event
/// observers.
///
/// `#[allow(dead_code)]` because callers live behind the
/// `cfg(any(test, feature = "test-support"))` gate (the `testing` facade in
/// `lib.rs` and `actor::tick`'s test module). A plain `cargo build` without
/// `--tests` or the `test-support` feature would otherwise warn.
#[allow(dead_code)]
pub fn run_actor(
    command_rx: Receiver<ActorCommand>,
    // Self-feedback sender — see `run_actor_with_observers` for the
    // contract. The backwards-compat shim threads it through unchanged.
    // Callers (tests + `lib.rs::spawn_actor`) hand in a clone of the
    // `Sender` they kept after constructing the channel.
    command_tx_self: Sender<ActorCommand>,
    update_tx: Sender<String>,
) {
    run_actor_with_observers(
        command_rx,
        command_tx_self,
        update_tx,
        new_lifecycle_observer_slot(),
        new_event_observer_slot(),
        new_raw_event_observer_slot(),
        crate::kernel::new_snapshot_projection_slot(),
        // D0: NIP-47 NWC is an app noun — this backwards-compatible entry
        // point has no FFI surface to register the `"wallet"` projection, so
        // the slot is a private throwaway (no host reads it).
        #[cfg(feature = "wallet")]
        new_wallet_status_slot(),
        // D0: NIP-46 remote signing is an app noun — likewise a private
        // throwaway bunker-handshake slot (no FFI surface to register the
        // `"bunker_handshake"` projection here).
        new_bunker_handshake_slot(),
        // Typed slot constructor; the backwards-compatible entry
        // point has no FFI surface to read the slot, so it's a throwaway.
        crate::kernel::new_relay_edit_rows_slot(),
        Arc::new(Mutex::new(None)),
        // NIP-17 DM-inbox key slot — private throwaway: this backwards-compatible
        // entry point has no FFI surface for a `DmInboxProjection` to read it.
        Arc::new(Mutex::new(None)),
        new_capability_callback_slot(),
        Arc::new(Mutex::new(None)),
        // G-S4 — no `NmpApp` is wired through this backwards-compatible entry
        // point, so the queue-depth counter is a private throwaway.
        Arc::new(AtomicU64::new(0)),
    );
}

/// T118 / G3 backwards-compatible entry point. Spawns the actor with a
/// lifecycle observer but no kernel event observer slot — the latter
/// defaults to an empty slot (nothing fans out, zero overhead). New
/// integrations should prefer [`run_actor_with_observers`] so kernel-event
/// fan-out is wired.
#[allow(dead_code)]
pub fn run_actor_with_lifecycle_observer(
    command_rx: Receiver<ActorCommand>,
    // Self-feedback sender — see `run_actor_with_observers`.
    command_tx_self: Sender<ActorCommand>,
    update_tx: Sender<String>,
    lifecycle_observer: LifecycleObserverSlot,
) {
    run_actor_with_observers(
        command_rx,
        command_tx_self,
        update_tx,
        lifecycle_observer,
        new_event_observer_slot(),
        new_raw_event_observer_slot(),
        crate::kernel::new_snapshot_projection_slot(),
        // D0: NIP-47 NWC is an app noun — no FFI surface here to register the
        // `"wallet"` projection, so the slot is a private throwaway.
        #[cfg(feature = "wallet")]
        new_wallet_status_slot(),
        // D0: NIP-46 remote signing is an app noun — private throwaway
        // bunker-handshake slot (no FFI surface here).
        new_bunker_handshake_slot(),
        // Typed slot constructor; private throwaway here.
        crate::kernel::new_relay_edit_rows_slot(),
        Arc::new(Mutex::new(None)),
        // NIP-17 DM-inbox key slot — private throwaway: no FFI surface here.
        Arc::new(Mutex::new(None)),
        new_capability_callback_slot(),
        Arc::new(Mutex::new(None)),
        // G-S4 — no `NmpApp` is wired through this backwards-compatible entry
        // point, so the queue-depth counter is a private throwaway.
        Arc::new(AtomicU64::new(0)),
    );
}

/// T118 / G3 + T146 — actor entry point that accepts BOTH the lifecycle
/// observer slot and the kernel event observer slot. The FFI
/// (`ffi/lifecycle.rs::nmp_app_set_lifecycle_callback`,
/// `ffi/event_observer.rs::nmp_app_register_event_observer`) shares the SAME
/// `Arc<Mutex<…>>` instances so registrations from outside the actor are
/// visible without crossing the FFI on each event.
///
/// Dual-channel priority design: `command_rx` is drained via `try_recv` at
/// the top of every iteration so UI commands are NEVER dropped under relay
/// event flood. The drain is budgeted so relay events and idle work still
/// progress under sustained command bursts. Relay events use a separate
/// channel read with `recv_timeout(compute_wait(…))` so emit-hz cadence is
/// respected when the command lane is not saturated.
#[allow(clippy::too_many_arguments)]
pub fn run_actor_with_observers(
    command_rx: Receiver<ActorCommand>,
    // Self-feedback sender — a clone of `command_rx`'s upstream `Sender`,
    // handed to dispatch arms that spawn background workers (currently the
    // `FetchLnurlInvoice` LNURL-pay HTTP round-trip). The worker uses it to
    // send a follow-up `ActorCommand` (e.g. `ShowToast` with the bolt11)
    // back into this loop without needing access to the `NmpApp`. The actor
    // itself never `recv`s on this sender — it only hands clones out via
    // `ActorContext::command_tx_self`.
    command_tx_self: Sender<ActorCommand>,
    update_tx: Sender<String>,
    lifecycle_observer: LifecycleObserverSlot,
    event_observers: KernelEventObserverSlot,
    raw_event_observers: RawEventObserverSlot,
    // Host-extensible snapshot output slot. Shared `Arc` with the `NmpApp`:
    // the C-ABI `nmp_app_register_snapshot_projection` mutates registrations
    // through one clone (host init); this actor thread binds the other onto
    // the kernel so `make_update` reads the same registry without crossing
    // FFI on each tick.
    snapshot_projections: crate::kernel::SnapshotProjectionSlot,
    // D0: NIP-47 NWC is an app noun — the shared wallet-status slot. One `Arc`
    // clone is captured by the `"wallet"` snapshot-projection closure on the
    // `NmpApp`; this one is handed to the actor's `WalletRuntime`, which is the
    // sole writer (D4). Gated behind the `wallet` feature so the
    // protocol-neutral build carries no NWC plumbing.
    #[cfg(feature = "wallet")] wallet_status: WalletStatusSlot,
    // D0: NIP-46 remote signing is an app noun — the shared bunker-handshake
    // slot. One `Arc` clone is captured by the built-in `"bunker_handshake"`
    // snapshot-projection closure on the `NmpApp`; this one is handed to the
    // actor's `IdentityRuntime`, which is the sole writer (D4).
    bunker_handshake: BunkerHandshakeSlot,
    // Typed slot ([`crate::kernel::RelayEditRowsSlot`]) so the actor
    // parameter type signals the slot's purpose; D14 forbids new bare
    // `Arc<Mutex<Vec<…>>>` parameters here.
    relay_edit_rows: crate::kernel::RelayEditRowsSlot,
    marmot_local_nsec: Arc<Mutex<Option<zeroize::Zeroizing<String>>>>,
    // NIP-17 DM-inbox decryption key seam. Shared `Arc` with the `NmpApp`:
    // per-app crates read the slot through `NmpApp::nip17_local_keys`; this
    // actor thread is the sole writer, updating it on every identity mutation
    // (parallel to `marmot_local_nsec`).
    nip17_local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    capability_callback: CapabilityCallbackSlot,
    // FFI-supplied persistent LMDB storage path. Shared `Arc` with the
    // `NmpApp`: the C-ABI `nmp_app_set_storage_path` writes through one
    // clone before `nmp_app_start`; this actor thread reads the other when
    // it constructs the kernel below. `None` (the test / web default)
    // keeps the in-memory store.
    storage_path: Arc<Mutex<Option<String>>>,
    // G-S4 — actor command-channel depth straddle counter. Shared `Arc` with
    // the `NmpApp`: `send_cmd` does `fetch_add(1)` before every channel send;
    // this actor thread does `fetch_sub(1)` per dequeued command and binds the
    // handle onto the kernel so `make_update` surfaces `actor_queue_depth`.
    queue_depth: Arc<AtomicU64>,
) {
    // Dual-channel design: relay events get their own dedicated channel.
    // No merged SyncSender<ActorMsg>, no forwarder threads, no drops.
    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();

    // T114b — bind a dispatch-drops counter for diagnostic visibility. Under
    // the new dual-channel design the counter is always zero (commands cannot
    // be dropped), but the kernel API and the Reset rebind path are kept so
    // the FFI surface and diagnostic snapshot don't change.
    let dispatch_drops = Arc::new(AtomicU64::new(0));

    // Wait for the first command before constructing the kernel. `nmp_app_new`
    // starts this actor thread immediately, while the host sets the LMDB path
    // through `nmp_app_set_storage_path` right after creating the handle and
    // before `Start`. Blocking here removes that init-order race without
    // polling; the first command is replayed through the normal dispatch path
    // below after the kernel has been built with the latest path.
    let first_command = match command_rx.recv() {
        Ok(ActorCommand::Shutdown) | Err(_) => return,
        Ok(command) => command,
    };

    // Resolve the FFI-supplied storage path once, after at least one host
    // command has reached the actor. If the slot is still empty — or the lock
    // is poisoned — the kernel falls back to the in-memory store. The
    // `lmdb-backend` feature gate lives inside `build_event_store`; this path
    // is plumbed unconditionally.
    let initial_storage_path: Option<String> =
        storage_path.lock().ok().and_then(|guard| guard.clone());
    let mut kernel =
        Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, initial_storage_path.as_deref());
    // T114b — bind the FFI-channel drop counter so it surfaces on the
    // diagnostic snapshot (`Metrics::dispatch_drops_total`). A `Reset`
    // command replaces the kernel; we re-bind there so the counter stays
    // visible (the underlying `Arc<AtomicU64>` survives Reset).
    kernel.set_dispatch_drops_handle(Arc::clone(&dispatch_drops));
    // G-S4 — bind the actor command-channel depth counter so it surfaces on
    // the diagnostic snapshot (`Metrics::actor_queue_depth`). `NmpApp::send_cmd`
    // increments it; this loop decrements per dequeued command (both recv
    // sites below). Survives `Reset` the same way the drop counter does —
    // re-bound there so the counter stays visible across a kernel rebuild.
    kernel.set_queue_depth_handle(Arc::clone(&queue_depth));
    // T146 — bind the shared kernel event observer slot. The kernel calls
    // `notify_event_observers` after every `EventStore::insert` returning
    // `Inserted | Replaced` (see `kernel/ingest/timeline.rs`). Per-app
    // crates (e.g. `nmp-app-chirp`) clone this slot via
    // `NmpApp::register_event_observer` to register typed observers.
    // Survives `Reset` the same way the drop counter does.
    kernel.set_event_observers_handle(Arc::clone(&event_observers));
    // Bind the shared raw signed-event tap slot. The kernel calls
    // `notify_raw_observers` from the single all-kinds ingest point
    // (`kernel/ingest/mod.rs::handle_event`) after the event passes the
    // existing Schnorr + id-hash gate, for any kind a registration filters
    // on. Survives `Reset` the same way the event-observer slot does so
    // external registrations stay live across a kernel rebuild.
    kernel.set_raw_event_observers_handle(Arc::clone(&raw_event_observers));
    // Bind the shared snapshot-projection slot. The kernel runs every
    // host-registered projection closure in `make_update` and appends the
    // result to `KernelSnapshot::projections`. Per-app crates register
    // through the C-ABI `nmp_app_register_snapshot_projection`, which mutates
    // the same `Arc<Mutex<…>>`. Survives `Reset` the same way the other
    // shared handles do so host projections stay live across a kernel
    // rebuild.
    kernel.set_snapshot_projection_handle(Arc::clone(&snapshot_projections));
    // D0 — register the built-in `"bunker_handshake"` snapshot projection.
    // NIP-46 remote signing is an app noun, so handshake state is NOT a typed
    // `KernelSnapshot` field — it is projected under
    // `projections["bunker_handshake"]` exactly like a host-registered
    // namespace. The closure reads the shared bunker-handshake slot the
    // actor's `IdentityRuntime` writes; it runs on every snapshot tick (D8:
    // cheap, non-blocking — a single lock-and-clone). When no handshake is in
    // flight the slot holds `None` and the closure contributes JSON `null`,
    // preserving the "key present, value null when idle" semantic the host
    // sign-in flow decodes. Registered here (the actor wiring site) rather than
    // on the FFI surface so every actor consumer — FFI or test — gets it.
    {
        let projection_slot = Arc::clone(&bunker_handshake);
        if let Ok(mut registry) = snapshot_projections.lock() {
            registry.register("bunker_handshake", move || {
                // D6: a poisoned bunker-handshake mutex recovers via
                // `into_inner` rather than panicking inside the snapshot tick.
                let slot = projection_slot.lock().unwrap_or_else(|e| e.into_inner());
                slot.as_ref()
                    .map(|dto| serde_json::to_value(dto).unwrap_or(serde_json::Value::Null))
                    .unwrap_or(serde_json::Value::Null)
            });
        }
    }
    // D0 — second built-in NIP-46 projection: `"nip46_onboarding"`. Where
    // `"bunker_handshake"` carries the raw broker progress (stage string +
    // message), this projection carries the *typed* onboarding read model
    // shells render directly — the static signer-app probe table, the typed
    // `stage_kind`, and pre-computed `is_in_flight` / `is_failed` /
    // `is_terminal_success` / `can_cancel` flags. The closure reads the same
    // shared bunker-handshake slot the previous projection serializes, plus a
    // Rust-owned static signer-app list (no platform-shell ownership of
    // protocol-knowledge tables). Always present (never JSON null) so the host
    // can read `signer_apps` even when no handshake is in flight.
    {
        let projection_slot = Arc::clone(&bunker_handshake);
        if let Ok(mut registry) = snapshot_projections.lock() {
            registry.register("nip46_onboarding", move || {
                let dto = build_nip46_onboarding_dto(&projection_slot);
                serde_json::to_value(&dto).unwrap_or(serde_json::Value::Null)
            });
        }
    }
    // Bind the shared relay-edit rows handle so external Rust callers
    // (e.g. a per-app dispatch crate) can read the user's current
    // relay list without crossing FFI. Survives `Reset` the same way as
    // the other shared handles.
    kernel.set_relay_edit_rows_handle(Arc::clone(&relay_edit_rows));
    // D4: the identity runtime is the sole writer of the shared
    // bunker-handshake slot. The built-in `"bunker_handshake"` snapshot
    // projection registered above reads the same `Arc<Mutex<…>>` clone on
    // every tick.
    let mut identity = IdentityRuntime::new(bunker_handshake);
    // D4: the wallet runtime is the sole writer of the shared wallet-status
    // slot. The `"wallet"` snapshot projection (registered on `NmpApp`) reads
    // the same `Arc<Mutex<…>>` clone on every tick.
    #[cfg(feature = "wallet")]
    let mut wallet = WalletRuntime::new(wallet_status);
    // T105: URL-keyed transport pool. One socket per resolved relay URL;
    // workers spawn on demand as OutboundMessages flow with new relay_urls.
    // Keyed by `CanonicalRelayUrl` so the canonicalization invariant is
    // compiler-enforced — a raw `&str` cannot index the pool.
    let mut relay_controls: HashMap<CanonicalRelayUrl, RelayControl> = HashMap::new();
    let mut connected_relays = HashSet::new();
    let mut connected_urls: HashSet<CanonicalRelayUrl> = HashSet::new(); // T116/G1 reconnect-replay discriminator.
    let mut next_relay_generation = 1;
    let mut running = false;
    let mut emit_hz = DEFAULT_EMIT_HZ;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut startup_sent = false;
    // Remote (NIP-46) sign ops parked off the blocking path. `dispatch_command`
    // pushes a `PendingSign` when a publish-command sign goes `Pending`; the
    // idle section below `poll()`s each one per tick and publishes on
    // completion. Lives outside the loop so parked ops survive across ticks.
    let mut pending_signs: Vec<PendingSign> = Vec::new();
    let mut queued_publish_outbound = Vec::new();
    let mut first_command = Some(first_command);

    loop {
        // ── Priority lane: commands ──────────────────────────────────────
        // Drain a bounded burst of pending commands before touching relay
        // events. Commands still get first service on every iteration, but the
        // budget prevents a sustained command stream from starving relay
        // events, subscription ticks, publish retries, and parked sign ops.
        let mut command_drain = CommandDrain::new(COMMAND_DRAIN_BUDGET);
        loop {
            if !command_drain.can_drain_command() {
                break;
            }
            let command_result = if let Some(command) = first_command.take() {
                Ok(command)
            } else {
                command_rx.try_recv()
            };
            match command_result {
                Ok(command) => {
                    command_drain.record_command();
                    // G-S4 — straddle counter: one command has left the channel
                    // (either the replayed `first_command`, which `command_rx
                    // .recv()` already dequeued, or a fresh `try_recv`). Mirror
                    // `NmpApp::send_cmd`'s `fetch_add(1)` so the depth tracks
                    // occupancy. `saturating_sub` guards the (benign) race where
                    // the actor drains a command sent through `actor_sender`,
                    // which bypasses the increment. `Relaxed` — observability,
                    // not synchronization.
                    queue_depth
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                            Some(d.saturating_sub(1))
                        })
                        .ok();
                    // Bundle the actor's mutable runtime state into a borrowed
                    // `ActorContext` for the duration of this one dispatch.
                    // Built fresh per command and dropped immediately after, so
                    // every other call site in this loop keeps using the
                    // original locals untouched (no loop-lifetime borrow).
                    let relays_ready = all_relays_connected(&connected_relays);
                    let mut ctx = ActorContext {
                        kernel: &mut kernel,
                        identity: &mut identity,
                        #[cfg(feature = "wallet")]
                        wallet: &mut wallet,
                        relay_controls: &mut relay_controls,
                        relay_tx: &relay_tx,
                        connected_relays: &mut connected_relays,
                        connected_urls: &mut connected_urls,
                        update_tx: &update_tx,
                        last_emit: &mut last_emit,
                        next_relay_generation: &mut next_relay_generation,
                        running: &mut running,
                        emit_hz: &mut emit_hz,
                        startup_sent: &mut startup_sent,
                        relays_ready,
                        lifecycle_observer: &lifecycle_observer,
                        marmot_local_nsec: &marmot_local_nsec,
                        nip17_local_keys: &nip17_local_keys,
                        capability_callback: &capability_callback,
                        pending_signs: &mut pending_signs,
                        command_tx_self: &command_tx_self,
                    };
                    let outbound = dispatch_command(command, &mut ctx);
                    let Some(outbound) = outbound else {
                        return; // Shutdown
                    };
                    route_dispatch_outbound(
                        running,
                        &mut queued_publish_outbound,
                        &mut relay_controls,
                        &relay_tx,
                        &mut kernel,
                        &mut next_relay_generation,
                        outbound,
                    );
                    if running
                        && maybe_send_startup(
                            running,
                            &mut startup_sent,
                            &connected_relays,
                            &mut relay_controls,
                            &relay_tx,
                            &mut kernel,
                            &mut next_relay_generation,
                        )
                    {
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    close_relays(&mut relay_controls, &mut connected_relays, &mut kernel);
                    connected_urls.clear();
                    return;
                }
            }
        }

        // ── Relay event lane ─────────────────────────────────────────────
        // Block up to compute_wait so emit-hz is respected without busy-spin.
        let wait = command_drain.relay_wait(compute_wait(&kernel, running, last_emit, emit_hz));
        match relay_rx.recv_timeout(wait) {
            Ok(event) => {
                // The pool is keyed by `CanonicalRelayUrl`; a relay worker is
                // spawned with — and reports events under — its canonical key,
                // so `parse_or_raw` round-trips to the same map entry.
                let relay_url = CanonicalRelayUrl::parse_or_raw(event.relay_url());
                let generation = event.generation();
                if relay_controls
                    .get(&relay_url)
                    .is_none_or(|control| control.generation != generation)
                {
                    // Stale event from a disposed worker — ignore.
                } else {
                    // Reliability north star: `handle_relay_event` processes
                    // arbitrary bytes from the network — it is the highest-risk
                    // panic site in the actor. Wrap it in `catch_unwind` so a
                    // panic in relay frame processing cannot kill the kernel:
                    // the actor loop survives, logs the payload, surfaces an
                    // error toast, and processes the next event fresh.
                    //
                    // `AssertUnwindSafe` is required because the closure
                    // captures `&mut` kernel state (`HashMap`/`Mutex` interiors
                    // are not `UnwindSafe`). This is sound here: the actor is
                    // single-threaded, so there is no other thread that could
                    // observe partially-mutated / poisoned state. Per D1
                    // (best-effort rendering) the kernel tolerates partial
                    // state — the invariant we protect is loop survival, not
                    // per-event atomicity.
                    //
                    // The command drain above is deliberately NOT wrapped:
                    // commands are internally generated, so a panic there is a
                    // genuine bug that must stay visible.
                    let result = panic::catch_unwind(AssertUnwindSafe(|| {
                        handle_relay_event(
                            event,
                            &mut kernel,
                            #[cfg(feature = "wallet")]
                            &mut wallet,
                            &mut relay_controls,
                            &relay_tx,
                            &mut next_relay_generation,
                            &mut connected_relays,
                            &mut connected_urls,
                            &update_tx,
                            &mut last_emit,
                            &mut startup_sent,
                            running,
                        );
                    }));
                    if let Err(panic_payload) = result {
                        let msg = panic_payload
                            .downcast_ref::<&str>()
                            .map(|s| s.to_string())
                            .or_else(|| panic_payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "unknown panic".to_string());
                        kernel.log(format!("actor: relay event handler panicked: {msg}"));
                        kernel.set_last_error_toast(Some(
                            "relay processing error — continuing".to_string(),
                        ));
                        // Surface the toast on this tick rather than waiting
                        // for the next `flush_due` — mirrors the pending-sign
                        // error path below.
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                }
            }
            Err(_timeout_or_disconnected) => {
                // Timeout (normal idle tick) or relay_rx disconnected (actor
                // holds relay_tx so this can't happen in practice). Either way
                // fall through to idle work below.
            }
        }

        // ── Idle work (runs on every iteration after relay poll) ─────────
        // Flush any time-gated view requests (e.g. contacts_deadline) and
        // run the M2 planner tick only while the actor is running. Before
        // Start these would spawn relay workers (via send_all_outbound) and
        // trigger relay-lifecycle events that emit spurious snapshots on the
        // update channel even though no consumer is listening — the root
        // cause of the S2 retention leak (T114b / s2-retention-audit.md).
        // The publish engine tick below already carries the same running gate
        // for the same reason. Pending profile claims, deferred view
        // requests, and lifecycle triggers all survive in kernel state until
        // Start flushes them through spawn_missing_relays + the first
        // running-gated idle tick.
        if running {
            let pending = kernel.pending_view_requests();
            if !pending.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &relay_tx,
                    &mut kernel,
                    &mut next_relay_generation,
                    pending,
                );
            }
        }
        // T142 — M2 planner tick: drain the subscription lifecycle's trigger
        // inbox. Per D8, an empty inbox is a zero-cost no-op (single
        // `is_empty()` check — no allocation, no compile pass). When
        // triggers are queued (e.g. FollowListChanged A11, Nip65Arrived A1)
        // this produces REQ/CLOSE WireFrames that are converted to
        // OutboundMessages and sent to the relay pool. Placed after M1
        // `pending_view_requests()` to ensure M1 CLOSE frames are enqueued
        // before M2 opens new subs (spec §3.1 placement rationale).
        if running {
            let wire_frames = kernel.drain_lifecycle_tick();
            if !wire_frames.is_empty() {
                let outbound = wire_frames_to_outbound(wire_frames, &mut kernel);
                send_all_outbound(
                    &mut relay_controls,
                    &relay_tx,
                    &mut kernel,
                    &mut next_relay_generation,
                    outbound,
                );
            }
        }
        // T127: actor-tick for the publish engine. The 250ms idle poll
        // in `compute_wait` (`tick.rs`) already paces this; no
        // additional throttle (the engine's own pending_retries gate
        // skips dispatch work when nothing is due). D8 — when
        // `in_flight` is empty the tick is heap-free:
        //   - `PublishEngine::tick` collects `Vec<PublishHandle>`
        //     from an empty iterator (Rust's `FromIterator for Vec`
        //     special-cases empty → `Vec::new()`, no allocation),
        //   - `QueueDispatcher::drain` swaps in `Vec::new()` via
        //     `mem::take` (no allocation when the queue was empty),
        //   - the kernel returns `drained.into_iter().map(..).collect()`
        //     which is also heap-free for an empty source.
        // Closes Residual 1 from T117 — transient retries fire even
        // on a quiet socket (no inbound traffic).
        if running {
            let retry_frames = kernel.tick_publish_engine_for_now();
            if !retry_frames.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &relay_tx,
                    &mut kernel,
                    &mut next_relay_generation,
                    retry_frames,
                );
            }
        }
        // ── Poll parked NIP-46 remote sign ops ───────────────────────────
        // Non-blocking per D8: `SignerOp::poll` is a `try_recv`. Each parked
        // op is checked once per tick — completed ones publish their signed
        // event and are removed; timed-out ones surface a toast and are
        // removed; still-pending ones stay for the next tick. An empty
        // `pending_signs` makes this a single `Vec::retain_mut` over zero
        // items — heap-free, no false wakeups.
        if !pending_signs.is_empty() {
            pending_signs.retain_mut(|ps| {
                // Poll first: a result that landed on the same tick as the
                // deadline must not be lost to the timeout check.
                match ps.op.poll() {
                    None => {
                        if ps.timed_out() {
                            kernel.set_last_error_toast(Some("remote sign timed out".to_string()));
                            // Broken-promise fix: a dispatched `PublishNote` /
                            // `PublishProfile` carries the registry-minted
                            // `correlation_id` the host is waiting on. The
                            // broker never responded, so the publish never
                            // happens — record a terminal `"failed"` verdict so
                            // `action_results` clears the host spinner instead
                            // of leaving it to hang. Recorded BEFORE `emit_now`
                            // so this tick's snapshot drains it. `None` (a
                            // `react` / `follow` park) is a no-op — nothing is
                            // waiting on an id.
                            if let Some(id) = ps.correlation_id_override.clone() {
                                kernel
                                    .record_action_failure(id, "remote sign timed out".to_string());
                            }
                            // Surface the toast immediately rather than
                            // waiting up to one periodic flush tick —
                            // matches the success-path `emit_now` below.
                            if running {
                                emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                            }
                            false // Abandon — broker did not respond in time.
                        } else {
                            true // Still pending — keep for the next tick.
                        }
                    }
                    Some(Ok(signed)) => {
                        // Route via the target the op was parked with —
                        // `Auto` (NIP-65 outbox) for kind:1/3/7, `Explicit`
                        // for host-pinned action executors (NIP-29 group
                        // events). Without the parked target a bunker user's
                        // group event would silently revert to the outbox.
                        //
                        // Carry the parked `correlation_id_override` too: a
                        // dispatched `PublishNote` signed by a remote (NIP-46)
                        // broker must settle under the registry-minted id the
                        // host is waiting on, not the freshly signed event's
                        // id. `None` for every other parked publish.
                        let outbound = kernel.publish_signed_to_with_correlation(
                            &signed,
                            &ps.p_tags,
                            ps.target.clone(),
                            ps.correlation_id_override.clone(),
                        );
                        route_dispatch_outbound(
                            running,
                            &mut queued_publish_outbound,
                            &mut relay_controls,
                            &relay_tx,
                            &mut kernel,
                            &mut next_relay_generation,
                            outbound,
                        );
                        if running {
                            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        }
                        false // Done — remove.
                    }
                    Some(Err(e)) => {
                        let reason = format!("remote sign failed: {e}");
                        kernel.set_last_error_toast(Some(reason.clone()));
                        // Broken-promise fix: same as the timeout branch — a
                        // dispatched action's `correlation_id` must reach
                        // `action_results` as a terminal `"failed"` verdict so
                        // the host spinner clears. The broker rejected the sign
                        // (or its channel dropped), so the publish never
                        // happens. Recorded BEFORE `emit_now` so this tick's
                        // snapshot drains it; `None` is a no-op.
                        if let Some(id) = ps.correlation_id_override.clone() {
                            kernel.record_action_failure(id, reason);
                        }
                        // Surface the toast immediately rather than waiting
                        // up to one periodic flush tick — matches the
                        // success-path `emit_now` above.
                        if running {
                            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                        }
                        false // Done — remove.
                    }
                }
            });
        }
        // Only emit when state actually changed; do not emit on every
        // idle tick (D8: zero false-wakeup allocations after warmup).
        if flush_due(&kernel, running, last_emit, emit_hz) {
            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
        }
    }
}
