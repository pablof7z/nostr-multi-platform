//! T66a actor command handlers — identity / publish / relay-edit.
//!
//! ## D0 boundary
//!
//! The actor lives in `nmp-core`. `nmp-signers` depends on `nmp-core`, so
//! `nmp-core` CANNOT import `nmp-signers` (would be a dependency cycle).
//! `AccountManager` / `LocalKeySigner` / `Nip46Signer` therefore cannot be
//! used here. Instead the actor keeps a local `IdentityRuntime` of bare
//! `nostr::Keys` (for nsec/generated accounts) plus a map of
//! `Box<dyn RemoteSignerHandle>` (for NIP-46 / NIP-07 / hardware signers),
//! and adapts each active account to the kernel's existing `AuthSignerFn`
//! seam (`Kernel::bind_auth_signer`). `RemoteSignerHandle` is defined in
//! `crate::remote_signer` so the actor uses signers without importing the
//! `nmp-signers` crate; concrete impls live in `nmp-signers` and reach the
//! actor through the broker (below). Full `AccountManager` integration is
//! M14 (`UniFFI`), when the FFI surface can move to a crate that may depend
//! on both `nmp-core` and `nmp-signers`.
//!
//! ## NIP-46
//!
//! Doctrine D0 still forbids `nmp-core -> nmp-signers`, so the NIP-46
//! handshake (kind:24133 relay subscription, `connect/get_public_key` RPCs)
//! lives in a separate broker crate that depends on BOTH `nmp-core` and
//! `nmp-signers`. The actor's role is purely to host the `Box<dyn
//! RemoteSignerHandle>` once the broker has completed the handshake:
//!
//! - `ActorCommand::SignInBunker { uri }` — actor shape-validates the URI
//!   and seeds the identity runtime's bunker-handshake slot with
//!   `"connecting"`. The broker then drives the real handshake on its own
//!   relay client. D0: NIP-46 remote signing is an app noun, so handshake
//!   state is NOT a typed `KernelSnapshot` field — it is surfaced through the
//!   built-in `"bunker_handshake"` snapshot projection.
//! - `ActorCommand::BunkerHandshakeProgress { stage, message }` — broker
//!   pushes progress (`"connecting"` → `"awaiting_pubkey"` → `"ready"` /
//!   `"failed"`); the actor reflects it into the bunker-handshake slot the
//!   `"bunker_handshake"` projection reads.
//! - `ActorCommand::AddRemoteSigner { handle }` — once the handshake
//!   completes (the broker has the user's pubkey from `get_public_key`),
//!   it hands the fully-initialized handle to the actor. The actor
//!   inserts it into `IdentityRuntime.remote_signers`, becomes active if
//!   no account was active, and routes all subsequent `sign_active`
//!   through the handle's `sign(unsigned).wait(timeout)` call.
//!
//! The actor never imports `nmp-signers`; it only touches the trait. NIP-42
//! is currently cleared while a remote signer is active (the broker's
//! ephemeral key cannot sign NIP-42 challenges as the user); the limitation
//! is surfaced to the user via a toast (V-06 Stage 1). Routing NIP-42
//! through the remote signer is tracked as V-06 Stages 2-3 in BACKLOG.

// Test-support facade for the NIP golden-tag conformance suite. Gated so it is
// never compiled into a production build. Exposed up the actor module chain to
// `lib.rs::testing` so the `tests/nip_tag_conformance.rs` integration test can
// drive the (otherwise `pub(crate)`) command handlers.
// `conformance_support` drives the native publish/dm command helpers — it
// shares the native-runtime gate with those submodules. V-01 Phase 1c.
#[cfg(all(any(test, feature = "test-support"), feature = "native"))]
mod conformance_support;
// V-01 Phase 1c: these handler submodules sit on the native actor runtime
// (they consume `PendingSign`, drive the publish engine, run the LNURL HTTP
// worker, etc.). Gated behind `native` to match `mod relay_worker` and the
// `pub fn run_actor*` family in `actor/mod.rs`. The observer slots
// (`event_observer`, `raw_event_observer`, `lifecycle`) stay always-compiled
// because the FFI surface and per-app crates name those types without
// requiring the native runtime to be present.
// V-39: NIP-17 DM send orchestration moved to `nmp-nip17` (see
// `crates/nmp-nip17/src/dm_send.rs::SendGiftWrappedDmCommand`). The
// `ActorCommand::SendGiftWrappedDm` variant + the `commands::dm` module are
// deleted; the equivalent path now dispatches `ActorCommand::Protocol(
// Box::new(SendGiftWrappedDmCommand { ... }))`.
mod event_observer;
mod identity;
mod lifecycle;
#[cfg(feature = "native")]
mod publish;
mod raw_event_observer;
#[cfg(feature = "native")]
mod relays;
mod remote_signer_for_seal;
// V-41 — `zap` + `zap_lnurl` moved to
// `nmp_nip57::lnurl::FetchLnurlInvoiceCommand` (a `ProtocolCommand`
// dispatched via `ActorCommand::Protocol`). D0: `nmp-core` carries no
// LNURL HTTP code or NIP-57 nouns. The original files lived at
// `crates/nmp-core/src/actor/commands/zap.rs` + `zap_lnurl.rs`; their
// `commands::zap::tests` module migrated to
// `crates/nmp-nip57/src/lnurl/tests.rs`.
// D0: NIP-47 NWC is an app noun — the wallet command runtime (and its
// `nmp-nwc` dependency) is gated behind the `wallet` Cargo feature.
// V-01 Phase 1c: every test module below exercises the native actor
// runtime (publish / dm / relays helpers, `run_actor`, etc.). They share
// the `native` gate with the modules they drive.
#[cfg(all(test, feature = "native"))]
mod registration_seed_follow_tests;
#[cfg(all(test, feature = "native"))]
mod remote_signer_tests;
#[cfg(all(test, feature = "native"))]
mod t168_identity_followfeed_reconcile_tests;
#[cfg(all(test, feature = "native"))]
mod tests;
#[cfg(feature = "wallet")]
mod wallet;

// V-01 Phase 1c: identity command handlers sit on the native actor runtime.
#[cfg(feature = "native")]
pub(super) use identity::{
    add_remote_signer, bunker_handshake_progress, create_account, ensure_default_onboarding_relays,
    remove_account, restore_bunker_session, sign_in_bunker, sign_in_nsec, switch_active,
    IdentityRuntime,
};
// D0: NIP-46 remote signing is an app noun — the bunker-handshake slot + its
// constructor are re-exported (crate-wide) so the `ffi` module can build the
// shared slot and register the built-in `"bunker_handshake"` snapshot
// projection. `BunkerHandshakeDto` stays `identity`-private — callers drive it
// only through `bunker_handshake_progress` / `sign_in_bunker`.
// V-01 Phase 1c: bunker types consumed only by native FFI / actor runtime.
#[cfg(feature = "native")]
pub(crate) use identity::build_nip46_onboarding_dto;
// `new_bunker_handshake_slot` + `BunkerHandshakeSlot` reach `nmp-ffi` through
// `nmp_core::__ffi_internal::*`. The slot type is `#[doc(hidden)] pub` (the
// inner `BunkerHandshakeDto` likewise) so `nmp_app_new` can construct an
// `Arc<Mutex<Option<BunkerHandshakeDto>>>` without re-implementing the slot
// shape — but the type stays out of the public docs.
#[cfg(feature = "native")]
pub use identity::{new_bunker_handshake_slot, BunkerHandshakeSlot};
// V-01 Phase 1c: lifecycle handler consumes the native dispatch path.
#[cfg(feature = "native")]
pub(super) use lifecycle::handle_lifecycle_event;
// V-01 Phase 1c: lifecycle slot/registration types consumed only by native FFI / actor runtime.
// `new_observer_slot` + `LifecycleObserverSlot` + `LifecycleObserverRegistration`
// are reached by `nmp-ffi` through `nmp_core::__ffi_internal::*` (after the
// step 11-final extraction).
#[cfg(feature = "native")]
pub use lifecycle::{
    new_observer_slot, LifecycleObserverRegistration, LifecycleObserverSlot,
};
// `pub` (not `pub(crate)`) so the test-support re-export in `lib.rs` works.
// `commands` is crate-private (`mod commands;`), so external Rust code only
// sees these through the gated `pub use` in lib.rs.
pub use lifecycle::{LifecycleObserverFn, LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};
// T146 — kernel event observer slot. Re-exported up the actor module chain so
// `ffi/event_observer.rs` and the per-app crate registration path (via
// `NmpApp::kernel_event_observers`) reach the same `Arc<Mutex<…>>` instance
// the kernel holds for fan-out.
// `KernelEventObserverSlot` and `notify_observers` are used by kernel/event_observer.rs
// unconditionally. The slot constructors and registration helpers are native FFI only.
pub(crate) use event_observer::notify_observers;
// `KernelEventObserverSlot` is reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::KernelEventObserverSlot`.
pub use event_observer::KernelEventObserverSlot;
// `register_c_observer` reaches `nmp-ffi` through
// `nmp_core::__ffi_internal::register_c_observer`.
#[cfg(feature = "native")]
pub use event_observer::register_c_observer;
// Slot constructor + Rust-side register/unregister helpers reach `nmp-ffi`
// through `nmp_core::__ffi_internal::*`.
#[cfg(feature = "native")]
pub use event_observer::{
    new_event_observer_slot, register_rust_observer, unregister_observer,
};
pub use event_observer::{
    KernelEventObserver, KernelEventObserverFn, KernelEventObserverId,
    KernelEventObserverRegistration,
};
// Raw signed-event tap. Parallel to the kernel-event observer slot above
// but delivers the verbatim flat NIP-01 signed event (`sig` included),
// kind-filtered. Generic capability (D0) — no protocol nouns. Re-exported
// up the actor chain so `ffi/raw_event_tap.rs` and the per-app crate
// registration path reach the same `Arc<Mutex<…>>` the kernel taps.
// V-39: `send_gift_wrapped_dm` re-export removed — moved to `nmp-nip17`.
#[cfg(feature = "native")]
pub(super) use publish::{
    follow, open_timeline, publish_note, publish_profile, publish_signed_event,
    publish_unsigned_event, publish_unsigned_event_to_relays, react,
};
// V-41 — `zap::handle_fetch_lnurl_invoice` was the legacy actor-thread
// LNURL handler. Deleted alongside the `FetchLnurlInvoice` `ActorCommand`
// variant. The replacement (`nmp_nip57::lnurl::FetchLnurlInvoiceCommand`)
// is a `ProtocolCommand` dispatched through `ActorCommand::Protocol`;
// `nmp-core` no longer carries the entry point.
pub(crate) use raw_event_observer::{notify_raw_observers, raw_observers_idle_for_kind};
// `register_c_raw_observer` reaches `nmp-ffi` through
// `nmp_core::__ffi_internal::register_c_raw_observer`.
pub use raw_event_observer::register_c_raw_observer;
// Slot constructor + Rust-side register/unregister helpers + slot type
// reach `nmp-ffi` through `nmp_core::__ffi_internal::*`.
pub use raw_event_observer::{
    new_raw_event_observer_slot, register_rust_raw_observer, unregister_raw_observer,
    RawEventObserverSlot,
};
pub use raw_event_observer::{
    KindFilter, RawEventObserver, RawEventObserverFn, RawEventObserverId,
    RawEventObserverRegistration,
};
// NIP golden-tag conformance harness — `pub` (not `pub(crate)`) so the gated
// test-support re-export in `lib.rs` reaches the integration test outside the
// crate. `commands` is itself crate-private, so non-test Rust code only sees
// this through `lib.rs::testing` when `feature = "test-support"` is on.
// V-01 Phase 1c: the harness sits on the native publish helpers, so the
// re-export shares the native gate with the submodule above.
#[cfg(all(any(test, feature = "test-support"), feature = "native"))]
pub use conformance_support::ConformanceHarness;
#[cfg(feature = "native")]
pub(super) use relays::{add_relay, build_relay_list_event_from_edit_rows, remove_relay};
#[cfg(feature = "wallet")]
pub(super) use wallet::{
    handle_nwc_text, wallet_connect, wallet_disconnect, wallet_pay_invoice, WalletRuntime,
};
// D0: NIP-47 NWC is an app noun — the wallet-status slot + its constructor are
// re-exported (crate-wide) so the `ffi` module can build the shared slot and
// register the `"wallet"` snapshot projection.
// `WalletStatusSlot` + `new_wallet_status_slot` reach `nmp-ffi` through
// `nmp_core::__ffi_internal::*`. The slot type is `#[doc(hidden)] pub`.
#[cfg(feature = "wallet")]
pub use wallet::{new_wallet_status_slot, WalletStatusSlot};
// `WalletStatus` is re-exported for the snapshot-projection test only, which
// constructs a status value when driving the `"wallet"` projection through
// `make_update`.
#[cfg(all(test, feature = "wallet"))]
pub(crate) use wallet::WalletStatus;
