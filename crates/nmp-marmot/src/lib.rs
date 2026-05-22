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
//!    All Marmot capabilities (key-package publish, group-scoped ops:
//!    `CreateGroup`, `InviteMember`, `SendMessage`, etc.) are covered by the
//!    bespoke `nmp_app_chirp_marmot_dispatch` C cluster (ADR-0025), not the
//!    generic `dispatch_action` seam. The previous `ActionModule` impls were
//!    deleted as dormant (zero registry callers); re-add only when a non-bespoke
//!    caller demands `dispatch_action` routing for a Marmot capability.
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

// NOTE: `nmp-marmot` exposes its 4 record types and its 4 view types as
// public types under `domain` and `view`. The view types are plain types
// whose `open` / `on_event_*` / `snapshot` inherent methods are reached via
// static dispatch — the `ViewModule` trait and the former
// `register(&mut ModuleRegistry)` entry point were both deleted because no
// kernel-side registry ever drove them. The live extension path is
// `KernelEventObserver` — see `nmp_core::substrate` module docs; the Marmot
// projection registers one in `projection/`. The previous `ActionModule`
// impls were deleted (6 group-scoped in PR #200, the last
// `PublishKeyPackageAction` shortly after); the bespoke
// `nmp_app_chirp_marmot_dispatch` C cluster (ADR-0025) covers every live
// Marmot capability today.

#[cfg(test)]
mod tests;
