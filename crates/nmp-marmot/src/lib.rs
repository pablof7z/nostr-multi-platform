//! `nmp-marmot` — Marmot Protocol (MLS-over-Nostr) as an NMP protocol crate.
//!
//! Adapter only — does NOT re-implement MLS. Wraps `mdk-core` 0.8.0 +
//! `mdk-sqlite-storage` 0.8.0. Spec: `docs/plan/marmot-mls.md`,
//! API surface: `docs/research/mdk-api.md`.
//!
//! ## Kernel-boundary exit gate
//!
//! - `nmp-core` gains zero MLS types. All MLS / MDK types stay inside this
//!   crate. `nmp-marmot` is the SOLE importer of `mdk-core` / `openmls`.
//! - No other NMP crate depends on MLS types. The substrate module impls
//!   (`domain` / `view` / `action`) expose only NMP-native record / payload /
//!   plan shapes; MDK types appear only in [`service`], which is consumed
//!   in-crate (tests) and by a future actor/FFI bridge.
//!
//! ## Storage seam — D4 (one writer per fact)
//!
//! Marmot deliberately runs a **second** store alongside the kernel's LMDB
//! event log. The two never write the same fact:
//!
//! | Store | Owner | Facts it is the SOLE writer of |
//! |-------|-------|--------------------------------|
//! | **MDK SQLite** (`mdk-sqlite-storage`, `<app_support>/marmot-mls-state.sqlite`) | [`service::MarmotService`] (this crate) | MLS ratchet state — group secrets, epoch tree, pending commits, processed-Welcome records, KeyPackage private keys. |
//! | **Kernel LMDB** (`nmp-core`) | the kernel actor / domain modules | Nostr wire events — the signed kind:30443/443/445/1059 envelopes, with their D10 source-relay provenance. |
//!
//! The ratchet state is private cryptographic material that MUST NOT live in
//! a shared event log; the wire events are public, replayable, and carry
//! provenance the MLS layer has no concept of. So the split is intrinsic,
//! not incidental. The invariant: **no fact is written to both stores.** A
//! kind:445 group message exists once in LMDB as an opaque ciphertext event
//! (kernel-owned) and its *decrypted* plaintext exists once in MDK SQLite
//! (service-owned) — different facts, one writer each. The
//! `domain` records ([`domain::MarmotGroupRecord`] et al.) are pure
//! read-projections derived from MDK SQLite; they are never the write path
//! and hold no ratchet state (see [`domain`] rustdoc). This crate is the
//! only code that opens the SQLite file, so D4's "one writer" holds by
//! construction — no other crate can write MLS state.
//!
//! ## Two-layer architecture
//!
//! 1. **Substrate module layer** ([`domain`], [`view`]) — mirrors `nmp-nip29`.
//!    Plain record + view types, exported as public types. These shapes carry
//!    NO MDK types — they satisfy the kernel-boundary grep.
//!    Marmot write capabilities (key-package publish, group-scoped ops:
//!    `CreateGroup`, `Invite`, `Send`, `Leave`, `Remove`, etc.) are dispatched
//!    through the substrate-generic [`projection::action::MarmotActionModule`]
//!    registered under the `"nmp.marmot"` namespace — the host calls
//!    `nmp_app_dispatch_action("nmp.marmot", action_json)` and the
//!    [`projection::handler::MarmotMlsOpHandler`] installed via
//!    `NmpApp::set_host_op_handler` runs the op against the live
//!    `MarmotProjection`. The legacy bespoke `nmp_marmot_dispatch` C symbol
//!    (ADR-0025) was DELETED in PR 3 (2026-05-23); the ADR-0025 exception
//!    is fully retired. In-process Rust callers that need the synchronous
//!    rich envelope use the Rust-native [`ffi::MarmotHandle::dispatch`]
//!    accessor (REPL / TUI / integration tests).
//! 2. **Service layer** ([`service::MarmotService`]) — the real MDK-driving
//!    API. Holds an `MDK<S>` + `nostr::Keys`. This is what the in-crate
//!    round-trip tests exercise and what a headless integration-test driver
//!    (and the future actor) hold. MDK is synchronous; callers in an async
//!    context offload via the runtime's blocking bridge (not this crate's
//!    concern — the service is sync `&self`).
//!
//! ## Welcome (kind:444) delivery + NIP-59
//!
//! The service drives NIP-59 gift-wrap / unwrap of kind:444 Welcome rumors
//! through `nmp_nip59::{gift_wrap, unwrap_gift_wrap}` (the M11.5 key-boundary
//! seam — caller holds `nostr::Keys`). NIP-59 stays a generic gift-wrap
//! crate with no Marmot nouns (D0); the Welcome-specific wiring lives here.
//!
//! ## Relay routing
//!
//! KeyPackage events (kind:30443/443) use standard author-write outbox
//! routing. Interest helpers live in [`interest`].

pub mod domain;
pub mod interest;
pub mod projection;
pub mod service;
pub mod view;

// ── C-ABI shell ──────────────────────────────────────────────────────────
//
// The `ffi` / `fetch` / `identity` / `credential_store` modules expose the
// `nmp_marmot_*` C-ABI symbols (plus the legacy `nmp_app_chirp_identity_*`
// names tracked for rename in Opus reviews #50 / #68). The surviving
// cluster (`nmp_marmot_register{,_active}`, `_snapshot`, `_group_messages`,
// `_string_free`, `_unregister`, `_fetch_key_packages`) is kernel-shaped
// per-app FFI (observer / projection / opaque-handle lifecycle) — NOT a
// `dispatch_action` violation; the ADR-0025 bespoke write-side dispatch
// (`nmp_marmot_dispatch`) was deleted in PR 3 (2026-05-23) and the ADR is
// retired. The C-ABI shell follows the same pattern Chirp's
// `nmp_app_chirp_*` cluster uses; Marmot lives at `crates/nmp-marmot/`
// (step 12 — returned from `apps/marmot/` 2026-05-25) as a Layer-4 NIP
// crate, with the per-app FFI cluster as its host-bridge surface.
// Chirp's iOS shell links the symbols transparently: `nmp-marmot` is
// pulled in as an `rlib`, and its `#[no_mangle]` symbols flow through
// `libnmp_app_chirp.a` (the staticlib the iOS target actually links
// against).
//
// Feature-gated behind `ffi` so consumers that only want the protocol
// types (in-process REPL, headless tests) do not have to pull in
// `keyring-core` / `apple-native-keyring-store`.
#[cfg(feature = "ffi")]
pub mod credential_store;
#[cfg(feature = "ffi")]
pub mod fetch;
#[cfg(feature = "ffi")]
pub mod ffi;
#[cfg(feature = "ffi")]
pub mod identity;

/// Re-exports of the handful of `mdk-core` types that appear in the public
/// [`service::MarmotService`] signature. Callers that drive the service
/// (round-trip tests in-crate; the diagnostic REPL out-of-crate) need to
/// construct these without taking a direct `mdk-core` dependency — the
/// kernel-boundary exit gate ("`nmp-marmot` is the SOLE importer of
/// `mdk-core`/`openmls`") would otherwise force every caller across the
/// boundary to import `mdk-core` themselves.
///
/// Add types here ONLY when they appear in `service`'s public API. This
/// module deliberately does NOT re-export the wider MDK surface.
pub mod mls_types {
    pub use mdk_core::prelude::{GroupId, MessageProcessingResult, NostrGroupConfigData};
}

// `nmp-marmot` exposes its 4 record types and 4 view types as public types
// under `domain` and `view`. View types are plain types reached via static
// dispatch; the live extension path is `KernelEventObserver` (the Marmot
// projection registers one in `projection/`). Write capabilities are
// dispatched through `projection::action::MarmotActionModule` registered
// under the `"nmp.marmot"` namespace; the legacy bespoke
// `nmp_marmot_dispatch` C cluster (ADR-0025) was DELETED in PR 3
// (2026-05-23) — the ADR-0025 exception is fully retired.

#[cfg(test)]
mod tests;
