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
//! actor through app/FFI composition. Full `AccountManager` integration is
//! M14 (`UniFFI`), when the FFI surface can move to a crate that may depend
//! on both `nmp-core` and `nmp-signers`.
//!
//! ## NIP-46
//!
//! Doctrine D0 still forbids `nmp-core -> nmp-signers`, so the NIP-46
//! handshake (kind:24133 relay subscription, `connect/get_public_key` RPCs)
//! lives in a separate app-neutral broker crate. The app/FFI adapter
//! translates completed broker outcomes into actor commands. The actor's role
//! is purely to host the `Box<dyn RemoteSignerHandle>` once the broker has
//! completed the handshake:
//!
//! - `ActorCommand::AddSigner { source: SignerSource::BunkerUri(uri), .. }` —
//!   actor shape-validates the URI and seeds the identity runtime's
//!   bunker-handshake slot with `"connecting"`. The broker then drives the real
//!   handshake on its own relay client. D0: NIP-46 remote signing is an app
//!   noun, so handshake state is NOT a typed `KernelSnapshot` field — it is
//!   surfaced through the built-in `"bunker_handshake"` snapshot projection.
//! - `ActorCommand::BunkerHandshakeProgress { stage, message }` — the adapter
//!   pushes broker progress (`"connecting"` → `"awaiting_pubkey"` →
//!   `"ready"` / `"failed"`); the actor reflects it into the
//!   bunker-handshake slot the
//!   `"bunker_handshake"` projection reads.
//! - `ActorCommand::AddSigner { source: SignerSource::RemoteHandle(handle), .. }`
//!   — once the handshake completes (the broker has the user's pubkey from
//!   `get_public_key`), it hands the fully-initialized handle to the actor. The
//!   actor inserts it into `IdentityRuntime.remote_signers`, applies the
//!   stashed `make_active` decision, and routes all subsequent signing
//!   through the handle's non-blocking `sign_active_nonblocking` path.
//!
//! The actor never imports `nmp-signers`; it only touches the trait. NIP-42
//! AUTH now routes through the remote signer via the ADR-0050 async signer port
//! (V-06 / #960): `sync_kernel` binds the AUTH *pubkey* for a remote account,
//! `handle_auth_challenge` parks the kind:22242, and the actor signs it through
//! the same `sign_*_nonblocking` seam as every other write — one uniform async
//! sign seam, no synchronous-broker bail.

// Test-support facade for the NIP golden-tag conformance suite. Gated so it is
// never compiled into a production build. Exposed up the actor module chain to
// `lib.rs::testing` so the `tests/nip_tag_conformance.rs` integration test can
// drive the (otherwise `pub(crate)`) command handlers.
// `conformance_support` drives the native publish/dm command helpers — it
// shares the native-runtime gate with those submodules. V-01 Phase 1c.
#[cfg(all(any(test, feature = "test-support"), feature = "native"))]
mod conformance_support;
// V-01 Phase 1c: these handler submodules sit on the native actor runtime
// (they consume `ParkedOp`, drive the publish engine, run the LNURL HTTP
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
// ADR-0052 §D3 — per-app signer-hook accessors on `IdentityRuntime`
// (`impl` block split out of `identity.rs` for file-size discipline).
mod signer_hooks;
// ADR-0032 / #1099: signer-state display label/tone derivation (split from
// `identity.rs` for file-size; mirrors the `cipher` split).
mod signer_state_label;
// ADR-0050 §D1 NIP-44 cipher helpers (split from `identity.rs` for file-size).
#[cfg(feature = "native")]
mod cipher;
mod lifecycle;
#[cfg(feature = "native")]
mod publish;
mod raw_event_observer;
#[cfg(feature = "native")]
mod relays;
// V-41 — `zap` + `zap_lnurl` moved to
// `nmp_nip57::lnurl::FetchLnurlInvoiceCommand` (a `ProtocolCommand`
// dispatched via `ActorCommand::Protocol`). D0: `nmp-core` carries no
// LNURL HTTP code or NIP-57 nouns. The original files lived at
// `crates/nmp-core/src/actor/commands/zap.rs` + `zap_lnurl.rs`; their
// `commands::zap::tests` module migrated to
// `crates/nmp-nip57/src/lnurl/tests.rs`.
// V-38 — the wallet command runtime moved to `crates/nmp-nip47`
// (`WalletRuntime` + the three NIP-47 NWC `ProtocolCommand` impls).
// `nmp-core` no longer depends on `nmp-nwc` and carries no NIP-47 nouns.
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

// V-01 Phase 1c: identity command handlers sit on the native actor runtime.
#[cfg(feature = "native")]
pub(super) use identity::{
    add_signer, bunker_connection_state_changed, bunker_handshake_progress, create_account,
    remove_account, restore_bunker_session, restore_nip55_session, switch_active, IdentityRuntime,
};
// ADR-0048 D6: the NIP-55 writer into the shared `signer_state` slot, driven
// by the `ActorCommand::Nip55SignerStateChanged` dispatch arm (Stage 2 —
// emitted by the nmp-ffi NIP-55 driver when the host capability bridge
// reports an outcome).
pub(super) use identity::nip55_signer_state_changed;
// D0: NIP-46 remote signing is an app noun — the bunker-handshake slot + its
// constructor are re-exported (crate-wide) so the `ffi` module can build the
// shared slot and register the built-in `"bunker_handshake"` snapshot
// projection. `BunkerHandshakeDto` stays `identity`-private — callers drive it
// only through `bunker_handshake_progress` / `add_signer`.
// V-01 Phase 1c: bunker types consumed only by native FFI / actor runtime.
#[cfg(feature = "native")]
pub(crate) use identity::build_nip46_onboarding_dto;
// D13 sign-and-return: the `SignEventForReturn` dispatch arm (`dispatch.rs`)
// reuses the same non-blocking sign helpers the publish path uses, calling
// them as `commands::sign_active_nonblocking` / `commands::sign_with_account_nonblocking`.
#[cfg(feature = "native")]
pub(super) use identity::{sign_active_nonblocking, sign_with_account_nonblocking};
// ADR-0050 §D1 — the cipher port verbs (`Nip44EncryptForAccount` /
// `Nip44DecryptForAccount`) reach these non-blocking NIP-44 helpers as
// `commands::nip44_encrypt_nonblocking` / `commands::nip44_decrypt_nonblocking`,
// the cipher siblings of the sign helpers above (in the `cipher` submodule to
// keep `identity.rs` within budget).
#[cfg(feature = "native")]
pub(super) use cipher::{nip44_decrypt_nonblocking, nip44_encrypt_nonblocking};
// `new_bunker_handshake_slot` + `BunkerHandshakeSlot` reach `nmp-ffi` through
// `nmp_core::__ffi_internal::*`. The slot type is `#[doc(hidden)] pub` (the
// inner `BunkerHandshakeDto` likewise) so `nmp_app_new` can construct an
// `Arc<Mutex<Option<BunkerHandshakeDto>>>` without re-implementing the slot
// shape — but the type stays out of the public docs.
#[cfg(feature = "native")]
pub use identity::{new_bunker_handshake_slot, BunkerHandshakeSlot};
// Test-only: the actor-owned typed-projection proof tests
// (`actor/typed_projections/typed_projections_tests.rs`) need to drive the
// shared slot to `Some(..)` to assert the conditional-presence behaviour.
// `BunkerHandshakeDto` stays production-private (above); this re-export is
// gated to test builds so it never widens the production surface.
#[cfg(all(test, feature = "native"))]
pub(crate) use identity::BunkerHandshakeDto;
// ADR-0048 D6: generalised remote-signer health slot + constructor.
// V-14 step b's `bunker_connection_state` slot is now `signer_state` — a
// hard-break rename (no compat aliases); all callers updated in the same PR.
#[cfg(feature = "native")]
pub use identity::{new_signer_state_slot, SignerStateSlot};
// Test-only: the typed-projection proof tests drive the shared slot to
// `Some(SignerStateDto)` to assert conditional presence — same gating as
// `BunkerHandshakeDto` above (the DTO stays production-private).
#[cfg(all(test, feature = "native"))]
pub(crate) use identity::SignerStateDto;
// ADR-0032 / #1099: the typed `signer_state` FlatBuffers decoder reuses this
// label/tone derivation as its forward-compat fallback for buffers that predate
// the tail-appended `status_label` / `status_tone` fields — one source of truth.
// Promoted to always-on: `decode_signer_state` is now `pub` (not test-only) so
// external shells can decode the typed sidecar from snapshot frames.
pub(crate) use signer_state_label::signer_state_label_and_tone;
// V-01 Phase 1c: lifecycle handler consumes the native dispatch path.
#[cfg(feature = "native")]
pub(super) use lifecycle::handle_lifecycle_event;
// V-01 Phase 1c: lifecycle slot/registration types consumed only by native FFI / actor runtime.
// `new_observer_slot` + `LifecycleObserverSlot` + `LifecycleObserverRegistration`
// are reached by `nmp-ffi` through `nmp_core::__ffi_internal::*` (after the
// step 11-final extraction).
#[cfg(feature = "native")]
pub use lifecycle::{new_observer_slot, LifecycleObserverRegistration, LifecycleObserverSlot};
// `pub` (not `pub(crate)`) so the test-support re-export in `lib.rs` works.
// `commands` is crate-private (`mod commands;`), so external Rust code only
// sees these through the gated `pub use` in lib.rs. The downstream re-export
// fires under `any(test, feature = "test-support")` (top-level) or
// `feature = "native"` (`__ffi_internal::LifecycleObserverFn`), so this gate
// is the union: anything narrower would leave the lib.rs imports unresolved.
#[cfg(any(test, feature = "test-support", feature = "native"))]
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
// Headless slot constructor — safe on wasm32 (no background thread).
// Used by `KernelReducer::new` on all targets.
pub(crate) use event_observer::new_event_observer_slot_headless;
// `register_rust_observer` is a pure-Rust helper with no native deps; it is
// available on all targets so wasm32 composition roots can register
// KernelEventObservers. `new_event_observer_slot` and `unregister_observer`
// remain native-only (used by the FFI / actor-thread shutdown path).
pub use event_observer::register_rust_observer;
// Slot constructor + unregister helper reach `nmp-ffi` through
// `nmp_core::__ffi_internal::*`.
#[cfg(feature = "native")]
pub use event_observer::{new_event_observer_slot, unregister_observer};
// `KernelEventObserver` / `KernelEventObserverFn` / `KernelEventObserverId`
// are the typed observer surface re-exported unconditionally from `lib.rs`
// (per-app Rust crates and the C-ABI wire shape). `KernelEventObserverRegistration`
// only reaches the outside world through `lib.rs::__ffi_internal`
// (`#[cfg(feature = "native")]`); gate it so a `--no-default-features` build
// does not see an unused-import on the registration type.
#[cfg(feature = "native")]
pub use event_observer::KernelEventObserverRegistration;
pub use event_observer::{KernelEventObserver, KernelEventObserverFn, KernelEventObserverId};
// Raw signed-event tap. Parallel to the kernel-event observer slot above
// but delivers the verbatim flat NIP-01 signed event (`sig` included),
// kind-filtered. Generic capability (D0) — no protocol nouns. Re-exported
// up the actor chain so `ffi/raw_event_tap.rs` and the per-app crate
// registration path reach the same `Arc<Mutex<…>>` the kernel taps.
// V-39: `send_gift_wrapped_dm` re-export removed — moved to `nmp-nip17`.
#[cfg(feature = "native")]
pub(super) use publish::{
    close_contact_feed, follow, open_contact_feed, publish_profile, publish_signed_event,
    publish_unsigned_event, publish_unsigned_event_to_relays, react,
};
// V-41 — `zap::handle_fetch_lnurl_invoice` was the legacy actor-thread
// LNURL handler. Deleted alongside the `FetchLnurlInvoice` `ActorCommand`
// variant. The replacement (`nmp_nip57::lnurl::FetchLnurlInvoiceCommand`)
// is a `ProtocolCommand` dispatched through `ActorCommand::Protocol`;
// `nmp-core` no longer carries the entry point.
pub(crate) use raw_event_observer::{notify_raw_observers, raw_observers_idle_for_kind};
// `register_c_raw_observer` reaches `nmp-ffi` through
// `nmp_core::__ffi_internal::register_c_raw_observer`. `__ffi_internal` is
// `#[cfg(feature = "native")]`; mirror the gate so a `--no-default-features`
// build does not see an unused re-export.
#[cfg(feature = "native")]
pub use raw_event_observer::register_c_raw_observer;
// Slot constructor + Rust-side register/unregister helpers reach `nmp-ffi`
// through `nmp_core::__ffi_internal::*` (same `native` gate). The
// `RawEventObserverSlot` type itself is consumed unconditionally by
// `kernel/raw_event_observer.rs` (the kernel holds an
// `Option<RawEventObserverSlot>` field), so the slot type re-export stays
// ungated.
pub use raw_event_observer::RawEventObserverSlot;
#[cfg(feature = "native")]
pub use raw_event_observer::{
    new_raw_event_observer_slot, register_rust_raw_observer, unregister_raw_observer,
};
// `KindFilter` / `RawEventObserver` / `RawEventObserverFn` / `RawEventObserverId`
// are the typed observer surface re-exported unconditionally from `lib.rs`.
// `RawEventObserverRegistration` reaches the outside world only through
// `lib.rs::__ffi_internal` (`#[cfg(feature = "native")]`); gate it to match.
#[cfg(feature = "native")]
pub use raw_event_observer::RawEventObserverRegistration;
pub use raw_event_observer::{
    KindFilter, RawEventObserver, RawEventObserverFn, RawEventObserverId,
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
pub(super) use relays::{add_relay, build_relay_list_event, remove_relay};
// V-38: wallet runtime + status slot moved to `crates/nmp-nip47`.
// `nmp-core` no longer has a `wallet` feature, a `wallet::` submodule, or
// any `WalletRuntime` / `WalletStatus` / `WalletStatusSlot` re-export.
