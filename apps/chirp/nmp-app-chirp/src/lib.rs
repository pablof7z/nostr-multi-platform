//! `nmp-app-chirp` — Chirp per-app glue.
//!
//! Composes `nmp-core` (the kernel substrate + event observer slot, T146)
//! with `nmp-nip01` (the NIP-10 modular timeline view) and `nmp-threading`
//! (the agnostic grouping algorithm) to surface Twitter-style stacked-reply
//! modules over the kernel's home timeline. Lives outside `nmp-core` because
//! ADR-0009 forbids `nmp-core -> nmp-nip01` (cycle).
//!
//! ## Wiring
//!
//! The iOS shell links this one aggregate static library for Chirp. Keeping
//! `nmp-ffi`, the NIP-46 broker adapter, and the Chirp projection in one Rust
//! archive gives the process exactly one copy of the native C-ABI state.
//!
//! The shell calls [`nmp_signer_broker_init`] once after `nmp_app_new` and
//! checks for `NmpConfigStatus::Ok`, then calls [`ffi::nmp_app_chirp_register`].
//! The projection registration:
//!
//! 1. Builds a reusable `nmp_nip01::ModularTimelineProjection` with the
//!    viewer's pubkey and the default `ModulePolicy`.
//! 2. Registers it as a kernel event observer via
//!    [`nmp_core::NmpApp::register_event_observer`]. From that moment on,
//!    every kind:1 the kernel ingests fans out to the projection.
//! 3. Returns an opaque handle the shell keeps for snapshots / unregister.
//!
//! The home feed is registered as the standard `"nmp.feed.home"` projection.
//! Render shells consume it from the normal NMP update stream and report
//! viewport intent through generic feed FFI; cursor math, page size, cap, and
//! quoted-card closure remain in reusable NMP crates.
//!
//! ## Doctrine
//!
//! * **D0** — kernel emits, this crate composes. No business logic in
//!   Swift; the grouping algorithm is in `nmp-threading`.
//! * **D6** — runtime FFI symbols degrade silently on null pointers, lock
//!   poisoning, or serialization failure; init-only config symbols return
//!   explicit status codes for ordering errors.

pub mod action_specs;
pub mod ffi;
pub mod snapshot_types;
pub mod typed_api;
#[cfg(feature = "wallet")]
mod wallet_runtime;

pub use ffi::{nmp_app_chirp_register, nmp_app_chirp_unregister, ChirpHandle, NmpRegisterStatus};
// ADR-0053 / Workstream-E4 — Chirp's projection-consumption intent: the single
// C-ABI call every Chirp shell (iOS, Android, tui, desktop) uses to declare it
// (`consume_all` — Chirp is a full client). No hand-maintained key list.
pub use ffi::nmp_app_chirp_declare_consumed_projections;
// #1493 — Chirp-owned create-account wrapper that injects Chirp's product seed
// follows (`nmp_chirp_config::chirp_default_follows`). The generic
// `nmp_app_create_new_account` auto-follows nobody; the Swift/Kotlin shells call
// THIS symbol so the seed pubkeys never transit the thin shell.
pub use ffi::nmp_app_chirp_create_new_account;
// M2 (ADR-0042 §5.1, V-112): per-open flat author/thread feeds that replace the
// `author_view`/`thread_view` projections + the four `open_*`/`close_*` symbols.
pub use action_specs::{
    action_spec_for_intent, action_spec_for_intent_json, action_spec_json_for_intent, follow_spec,
    publish_note_spec, publish_profile_spec, react_spec, repost_spec, send_dm_spec, unfollow_spec,
    zap_spec, ActionDispatchSpec, ChirpActionIntent, ReplyTargetInput,
};
pub use ffi::{
    nmp_app_chirp_close_author_feed, nmp_app_chirp_close_group_discovery,
    nmp_app_chirp_close_home_feed, nmp_app_chirp_close_tag_feed, nmp_app_chirp_close_thread_feed,
    nmp_app_chirp_open_author_feed, nmp_app_chirp_open_group_discovery,
    nmp_app_chirp_open_home_feed, nmp_app_chirp_open_tag_feed, nmp_app_chirp_open_thread_feed,
};
pub use nmp_ffi::{
    nmp_app_cancel_bunker_handshake, nmp_app_nostrconnect_uri, nmp_signer_broker_init,
};
pub use nmp_nip01::{
    Nip10ReplyAttribution as ChirpReplyAttribution, TimelineEventCard as ChirpEventCard,
};
pub use snapshot_types::{
    ActionResult, ActionStageRow, InterestRow, ProfileCard, RelayRow, RelayWireSubRow,
    RuntimeMetrics,
};
pub use typed_api::{
    create_account_action, follow_action, publish_note_action, publish_profile_action,
    react_action, remove_account_action, repost_action, send_dm_action, sign_in_nsec_action,
    switch_account_action, unfollow_action, zap_action, ChirpClient,
};

/// V-80 rung 7 / issue #1613 — the home-feed snapshot served under `"nmp.feed.home"`.
///
/// Was `nmp_nip01::ModularTimelineSnapshot` (`{ blocks, cards, … }`); now the
/// OP-centric [`nmp_feed::RootFeedSnapshot`] instantiated with the NIP-10
/// render card ([`ChirpEventCard`]) and the NIP-10 reply attribution
/// ([`ChirpReplyAttribution`]). Wire shape:
/// `{ "cards": [{ "card": ChirpEventCard, "attribution": [ChirpReplyAttribution] }], "page": …, "metrics": … }`.
///
/// Named `OpFeedSnapshot` to match the Swift type and the framework type
/// `nmp_nip01::op_feed::OpFeedSnapshot` — no app name in a framework type alias.
pub type OpFeedSnapshot = nmp_feed::RootFeedSnapshot<ChirpEventCard, ChirpReplyAttribution>;

// ── Marmot (MLS encrypted groups) projection ─────────────────────────────
//
// A second FFI projection over the same kernel substrate. Mirrors the
// timeline symbols' naming / lifetime / free conventions. The iOS agent
// links these alongside the timeline symbols.
//
// The reusable C-ABI shell lives in the `nmp-marmot` crate
// (`crates/nmp-marmot/src/ffi.rs` + siblings) so the crate is a standalone
// buildable target for a future Marmot-only app. Chirp pulls it in via the
// `nmp-marmot/ffi` feature; the `#[no_mangle] nmp_marmot_*` symbols flow
// through `libnmp_app_chirp.a` automatically via rlib linkage (iOS still
// links exactly one staticlib). Chirp-specific identity/keyring wrappers stay
// in this app crate so `nmp-marmot` does not own Chirp symbol names or
// keyring account policy.
//
// Gated behind the `marmot` feature: MLS-over-Nostr was formally deferred to
// post-v1. Chirp opts in via its default feature set; a no-default-features
// build excludes the whole projection (dependency, modules, and FFI symbols).
#[cfg(feature = "marmot")]
pub use ffi::{
    nmp_app_chirp_identity_remove_account, nmp_app_chirp_identity_restore,
    nmp_app_chirp_identity_sign_in_nsec,
};
// #1727: the vestigial `nmp_marmot_fetch_key_packages` C-ABI symbol was
// deleted — it had no native caller and the same key-package lookup interest
// is already pushed internally by the invite/group flow.
// V-107 / ADR-0039: `nmp_marmot_snapshot`, `nmp_marmot_group_messages`, and
// `nmp_marmot_string_free` were deleted. Swift now reads Marmot state from
// the push projections (`nmp.marmot.snapshot` / `nmp.marmot.messages`) on
// the SnapshotFrame instead. Only the lifecycle symbols remain exported here.
// #1727: `nmp_marmot_register` (secret-bearing) is no longer a C-ABI symbol —
// it became a plain Rust fn (`nmp_marmot::ffi::register_with_secret_hex`) used
// only Rust-side by the nsec sign-in wrapper. No native-facing `nmp_marmot_*`
// symbol carries secret key material; native registers via `register_active`,
// which reads the actor-owned `mls_local_nsec` slot.
#[cfg(feature = "marmot")]
pub use nmp_marmot::ffi::{nmp_marmot_register_active, nmp_marmot_unregister, MarmotHandle};
#[cfg(feature = "marmot")]
pub use nmp_marmot::projection::payload::{
    KeyPackageStatus, MarmotGroupRow, MarmotMessageRow, MarmotSnapshot, PendingWelcomeRow,
};
#[cfg(feature = "marmot")]
pub use nmp_marmot::projection::state::MarmotProjection;
