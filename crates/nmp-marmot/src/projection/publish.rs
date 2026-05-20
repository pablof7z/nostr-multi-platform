//! Internal relay-publish bridge — the CLOSED outbound Marmot seam.
//!
//! Historically the Marmot dispatch ops returned ready-to-publish signed
//! event JSON (`events` / `welcome_rumors` / `event`) for a Swift relay
//! layer to publish. No such Swift hook ever existed (the only Swift
//! publish symbol, `nmp_app_publish_unsigned_event`, signs kernel-side and
//! therefore cannot carry an MLS-credential-signed kind:445 or an
//! ephemeral-key gift-wrap verbatim — see `MarmotBridge.swift`'s KNOWN
//! LIMITATION note). The outbound events landed in local MDK SQLite but
//! never reached relays.
//!
//! This module closes that seam by publishing INTERNALLY through the
//! kernel: it calls the landed `nmp-core` capability symbol
//! [`nmp_app_publish_signed_event_to`] against the `*mut NmpApp` the host
//! shell's Marmot handle already retains. An empty relay set falls through
//! to the author NIP-65 outbox (`PublishTarget::Auto`), so the single
//! Explicit-target symbol covers both routing modes. It is a generic
//! kernel capability (no MLS/Marmot nouns kernel-side — D0 holds); it
//! verifies Schnorr + id and routes fire-and-forget via the actor channel.
//!
//! The inbound ingest seam (`{"op":"ingest_signed_event"}`) is a SEPARATE,
//! still-open seam (the `KernelEventObserver` fan-out is lossy — no
//! signature — so MDK cannot ingest from it). This module does not touch
//! that direction.
//!
//! ## Linkage
//!
//! The symbol is a `#[no_mangle] extern "C"` body inside `nmp-core`.
//! `nmp-core`'s Rust-path `pub use` of it is gated behind `test-support` /
//! `android-ffi`, so this crate cannot import it by path in a default
//! build. Instead we declare it in an `extern "C"` block: the linker
//! resolves the symbol from `libnmp_core` (the rlib in
//! `cargo test -p nmp-app-chirp`, the staticlib the iOS shell links). This
//! is exactly how a C consumer would reach it and needs no `nmp-core`
//! change.

use std::ffi::{c_char, c_void, CString};

use nmp_core::NmpApp;
use nostr::{Event, JsonUtil, RelayUrl};

// The `*mut NmpApp` is declared here as an opaque `*mut c_void`. `NmpApp`
// has unspecified layout (it is never accessed through this pointer on
// either side — the kernel reconstructs `&NmpApp` from it identically to
// its own FFI symbols), so a void pointer is the FFI-safe spelling and
// matches the byte-for-byte ABI of the `nmp-core`-side `#[no_mangle]`
// definitions (pointer identity is all that crosses).
extern "C" {
    /// `nmp-core` — verbatim publish to an explicit relay set
    /// (`PublishTarget::Explicit`). `relays_json` is a JSON array of
    /// relay-URL strings; empty / null → falls back to the author NIP-65
    /// outbox (`PublishTarget::Auto`).
    fn nmp_app_publish_signed_event_to(
        app: *mut c_void,
        event_json: *const c_char,
        relays_json: *const c_char,
    );
}

/// Publish a signed event to an explicit relay set (Explicit routing).
///
/// Used for relay-pinned kind:445 (group message / commit) and the
/// inbox-routing approximation for kind:1059 gift-wraps. An EMPTY `relays`
/// slice deliberately falls back to Auto (the kernel symbol's documented
/// empty-array behaviour) — callers use that as the "no group relays
/// cached" degradation path. No-op when `app` is null.
pub(crate) fn publish_to(app: *mut NmpApp, event: &Event, relays: &[RelayUrl]) {
    if app.is_null() {
        return;
    }
    let Ok(cjson) = CString::new(event.as_json()) else {
        return;
    };
    let relays_json = serde_json::to_string(
        &relays.iter().map(RelayUrl::to_string).collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());
    let Ok(crelays) = CString::new(relays_json) else {
        return;
    };
    // SAFETY: `app` is the live `*mut NmpApp` retained by the host's Marmot
    // handle for the handle's lifetime (same validity rule as the observer
    // register/unregister calls); the kernel symbol reconstructs `&NmpApp`
    // from the same pointer value (cast to/from `c_void` is
    // pointer-identity-preserving). Both C strings outlive the call. Empty
    // `relays` → `[]` → kernel Auto-fallback.
    unsafe {
        nmp_app_publish_signed_event_to(app.cast(), cjson.as_ptr(), crelays.as_ptr())
    }
}
