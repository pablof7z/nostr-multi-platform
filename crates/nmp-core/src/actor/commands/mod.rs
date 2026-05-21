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
//! M14 (UniFFI), when the FFI surface can move to a crate that may depend
//! on both `nmp-core` and `nmp-signers`.
//!
//! ## NIP-46
//!
//! Doctrine D0 still forbids `nmp-core -> nmp-signers`, so the NIP-46
//! handshake (kind:24133 relay subscription, connect/get_public_key RPCs)
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
//! - `ActorCommand::RemoveRemoteSigner { identity_id }` — broker (or UI)
//!   asks the actor to drop the handle (e.g. logout).
//!
//! The actor never imports `nmp-signers`; it only touches the trait. NIP-42
//! is currently cleared while a remote signer is active (the broker's
//! ephemeral key cannot sign NIP-42 challenges as the user); routing NIP-42
//! through the remote signer is a documented follow-up
//! (TODO(nip46-nip42) in `identity.rs:sync_kernel`).

// Test-support facade for the NIP golden-tag conformance suite. Gated so it is
// never compiled into a production build. Exposed up the actor module chain to
// `lib.rs::testing` so the `tests/nip_tag_conformance.rs` integration test can
// drive the (otherwise `pub(crate)`) command handlers.
#[cfg(any(test, feature = "test-support"))]
mod conformance_support;
mod event_observer;
mod identity;
mod lifecycle;
mod publish;
mod raw_event_observer;
mod relays;
// D0: NIP-47 NWC is an app noun — the wallet command runtime (and its
// `nmp-nwc` dependency) is gated behind the `wallet` Cargo feature.
#[cfg(test)]
mod registration_seed_follow_tests;
#[cfg(test)]
mod remote_signer_tests;
#[cfg(test)]
mod t168_identity_followfeed_reconcile_tests;
#[cfg(test)]
mod tests;
#[cfg(feature = "wallet")]
mod wallet;

pub(super) use identity::{
    add_remote_signer, bunker_handshake_progress, create_account, remove_account,
    remove_remote_signer, restore_bunker_session, sign_in_bunker, sign_in_nsec, switch_active,
    IdentityRuntime,
};
// D0: NIP-46 remote signing is an app noun — the bunker-handshake slot + its
// constructor are re-exported (crate-wide) so the `ffi` module can build the
// shared slot and register the built-in `"bunker_handshake"` snapshot
// projection. `BunkerHandshakeDto` stays `identity`-private — callers drive it
// only through `bunker_handshake_progress` / `sign_in_bunker`.
pub(crate) use identity::{new_bunker_handshake_slot, BunkerHandshakeSlot};
pub(super) use lifecycle::handle_lifecycle_event;
pub(crate) use lifecycle::{
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
pub(crate) use event_observer::{
    new_event_observer_slot, notify_observers, register_c_observer, register_rust_observer,
    unregister_observer, KernelEventObserverSlot,
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
pub(super) use publish::{
    follow, open_timeline, publish_note, publish_signed_event, publish_unsigned_event, react,
};
pub(crate) use raw_event_observer::{
    new_raw_event_observer_slot, notify_raw_observers, raw_observers_idle_for_kind,
    register_c_raw_observer, register_rust_raw_observer, unregister_raw_observer,
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
#[cfg(any(test, feature = "test-support"))]
pub use conformance_support::ConformanceHarness;
pub(super) use relays::{add_relay, remove_relay};
#[cfg(feature = "wallet")]
pub(super) use wallet::{
    handle_nwc_text, wallet_connect, wallet_disconnect, wallet_pay_invoice, WalletRuntime,
};
// D0: NIP-47 NWC is an app noun — the wallet-status slot + its constructor are
// re-exported (crate-wide) so the `ffi` module can build the shared slot and
// register the `"wallet"` snapshot projection.
#[cfg(feature = "wallet")]
pub(crate) use wallet::{new_wallet_status_slot, WalletStatusSlot};
// `WalletStatus` is re-exported for the snapshot-projection test only, which
// constructs a status value when driving the `"wallet"` projection through
// `make_update`.
#[cfg(all(test, feature = "wallet"))]
pub(crate) use wallet::WalletStatus;
