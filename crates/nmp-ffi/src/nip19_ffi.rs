//! H4 — NMP-provided NIP-19 identity-encoder FFI (`nmp_app_encode_profile`).
//!
//! Closes the H4 conformance finding: app shells were hand-rolling bech32 to
//! turn a hex pubkey into a display identifier. This module hands them a
//! single NMP-owned encoder so there is exactly one bech32 implementation in
//! the tree (`nmp_core::nip19`).
//!
//! ## What it does
//!
//! `pubkey_hex → nprofile | npub`:
//! - Prefers `nprofile` (pubkey + relay TLVs) when the kernel ALREADY holds
//!   the pubkey's kind:10002 relay hints — read out of the same
//!   `InMemoryMailboxCache` the `nmp_router::Kind10002Parser` writes on
//!   ingest (see [`crate::NmpApp::set_mailbox_cache_reader`] for the
//!   instance-identity contract that makes this branch live).
//! - Falls back to a bare `npub` when no relay hint is cached.
//!
//! ## What it never does
//!
//! NEVER fetches. It is a synchronous read of cached kind:10002 state — no
//! network, no actor round-trip, no async (no-polling / no-fetch doctrine).
//!
//! ## Doctrine map
//!
//! - **D0**: a generic identity encoder, no app noun — `nprofile` / `npub`
//!   are protocol primitives, not a product concept.
//! - **D5**: returns a `String` (a display identifier), never an event across
//!   FFI.
//! - **D6**: no `Result` / error crosses the boundary. Null / invalid input
//!   and any encode failure degrade gracefully to a heap copy of the raw
//!   input (or the empty string when truly unusable). Never panics.
//! - **no-fetch / no-polling**: cache read only.
//!
//! The encoders reused here live in `nmp_core::nip19` (`encode_npub`,
//! `encode_nprofile`, `NprofileData`). `nmp_core::display` is deliberately
//! NOT used — it is Rust-presentation-only and banned from FFI paths per
//! ADR-0032.

use crate::{app_ref, c_string_argument, NmpApp};
use nmp_core::nip19::{encode_npub, encode_nprofile, NprofileData};
use std::ffi::{c_char, CString};

/// `nprofile` TLV-encodes every relay it carries, so a full outbox set turns
/// into an absurdly long bech32 string. Cap the relay hints we embed; the
/// first few are sufficient for a resolver to find the author.
const MAX_NPROFILE_RELAYS: usize = 3;

/// Encode a hex pubkey as a NIP-19 display identifier — `nprofile1…` when the
/// kernel already holds the pubkey's kind:10002 relay hints, else `npub1…`.
///
/// `pubkey_hex` — 64-char lowercase hex public key (a C string).
///
/// Returns a heap `*mut c_char` the host MUST free via `nmp_free_string`.
/// D6: a null/invalid `app` or `pubkey_hex`, or any encode failure, degrades
/// to a heap copy of the raw input (empty string only when the input itself
/// is unusable) — never NULL, never a panic, never an error across the FFI.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_encode_profile(
    app: *mut NmpApp,
    pubkey_hex: *const c_char,
) -> *mut c_char {
    // Pull the raw pubkey first so every fallback path has something to return.
    let pubkey = c_string_argument(pubkey_hex).unwrap_or_default();
    let encoded = encode_profile(app_ref(app), &pubkey);
    into_c_string(encoded)
}

/// Pure encoder core — separated from the raw-pointer shell so the logic is
/// unit-testable without forging FFI pointers. Returns the bech32 string, or
/// the raw `pubkey` echoed back on any failure (D6 graceful fallback).
fn encode_profile(app: Option<&NmpApp>, pubkey: &str) -> String {
    // Relay hints, if any: read the SAME cache the Kind10002Parser writes.
    // `write_relays` returns the resolved write set (write + both, sorted);
    // `None` means "no kind:10002 cached for this author" → npub fallback.
    let relays = app
        .and_then(NmpApp::mailbox_cache_reader)
        .and_then(|cache| cache.write_relays(&pubkey.to_string()))
        .filter(|relays| !relays.is_empty());

    match relays {
        Some(mut relays) => {
            // nprofile TLV-encodes every relay; a full outbox set makes an
            // absurd bech32. Keep at most MAX_NPROFILE_RELAYS.
            relays.truncate(MAX_NPROFILE_RELAYS);
            let data = NprofileData {
                pubkey: pubkey.to_string(),
                relays,
            };
            encode_nprofile(&data).unwrap_or_else(|_| pubkey.to_string())
        }
        None => encode_npub(pubkey).unwrap_or_else(|_| pubkey.to_string()),
    }
}

/// Move a `String` into a heap C string the host frees via
/// `nmp_free_string`. An interior NUL (impossible for bech32 / hex, but
/// guarded for totality) collapses to the empty string — never a panic (D6).
fn into_c_string(value: String) -> *mut c_char {
    CString::new(value)
        .unwrap_or_else(|_| c"".to_owned())
        .into_raw()
}

#[cfg(test)]
#[path = "nip19_ffi_tests.rs"]
mod tests;
