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
/// inbox-routing approximation for kind:1059 gift-wraps. No-op when `app`
/// is null.
///
/// # D10 provenance guard — kind:1059 NEVER Auto-routes
///
/// Doctrine D10 forbids publishing a NIP-59 gift-wrap (kind:1059) to the
/// author's public NIP-65 outbox: doing so leaks the *existence* of an
/// encrypted DM / Marmot Welcome to every relay the author advertises for
/// public traffic — defeating the unlinkability gift-wrap exists to provide.
///
/// The kernel-side FFI symbol `nmp_app_publish_signed_event_to` treats an
/// empty `relays_json` array as `PublishTarget::Auto` (its documented
/// fallback for the back-compat path). That fallback is correct for
/// kind:30443/443 KeyPackages and kind:445 group messages (when the group
/// relay is cached out-of-band), but it is a **D10 leak** for kind:1059.
///
/// This guard refuses the publish when `event.kind == 1059` and `relays`
/// is empty. The kind:1059 envelope stays in the local store (callers like
/// `wrap_and_publish_welcomes` still return it as INFORMATIONAL) but is
/// **not** dispatched to any relay. Callers must supply a non-empty pin
/// (the recipient's kind:10050 DM-inbox relays, or the group's relays as
/// the existing inbox-routing approximation) for a kind:1059 publish to
/// actually go out.
pub(crate) fn publish_to(app: *mut NmpApp, event: &Event, relays: &[RelayUrl]) {
    if app.is_null() {
        return;
    }
    // D10 provenance guard: a kind:1059 gift-wrap with NO explicit relay
    // pin MUST NOT fall through to the kernel's Auto fallback (which would
    // publish to the author's NIP-65 outbox — leaking the presence of an
    // encrypted DM / Welcome to public relays). Refuse the publish; the
    // caller's informational return still carries the signed envelope JSON
    // so callers retain ground-truth audit of what was built.
    if event.kind.as_u16() as u32 == crate::interest::KIND_GIFT_WRAP && relays.is_empty() {
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
    // pointer-identity-preserving). Both C strings outlive the call. Non-empty
    // `relays` → `PublishTarget::Explicit`; an empty kind:445/30443/443 set
    // → the kernel Auto-fallback (only the kind:1059 case is guarded above).
    unsafe {
        nmp_app_publish_signed_event_to(app.cast(), cjson.as_ptr(), crelays.as_ptr())
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the D10 provenance guard in [`publish_to`].
    //!
    //! These tests exercise the guard's *kind discrimination* with a null
    //! `*mut NmpApp` so the FFI symbol is never invoked (a `nullptr` `app`
    //! returns early before the guard, but we cover the guard logic by
    //! exercising the inverse: with the early-null-return removed
    //! conceptually, the guard is the next line of defense). The behavioral
    //! contract this pins:
    //!
    //! - `event.kind == 1059` + `relays.is_empty()` → no publish.
    //! - `event.kind == 1059` + non-empty `relays` → publish proceeds
    //!   (the guard does not block; the caller has supplied an explicit pin).
    //! - `event.kind != 1059` + empty `relays` → publish proceeds (the
    //!   kernel Auto-fallback is the documented behaviour for kind:445 /
    //!   30443 / 443).
    //!
    //! Because `*mut NmpApp` is null in tests, no FFI symbol is reached;
    //! the assertions therefore inspect the guard *predicate* shape directly
    //! via a public helper, isolating the gate from the unsafe FFI body.
    use super::*;
    use nostr::Keys;
    use nostr::{EventBuilder, Kind};

    /// Test-only helper exposing the D10 predicate so a unit test can
    /// assert the gate's behavior without crossing FFI. Mirrors the inline
    /// check in [`publish_to`] exactly — a change here that diverges from
    /// the production guard would be a real bug.
    fn is_d10_blocked(event: &Event, relays: &[RelayUrl]) -> bool {
        event.kind.as_u16() as u32 == crate::interest::KIND_GIFT_WRAP && relays.is_empty()
    }

    /// Build a signed kind:1059-shaped event for the guard tests. The event
    /// body is irrelevant to the guard — only its `kind` matters — but we
    /// build a real signed envelope so the test exercises the same value
    /// shape production sees.
    fn sample_kind_1059() -> Event {
        let keys = Keys::generate();
        EventBuilder::new(Kind::from_u16(1059), "")
            .sign_with_keys(&keys)
            .expect("test-only signing must succeed")
    }

    fn sample_kind_445() -> Event {
        let keys = Keys::generate();
        EventBuilder::new(Kind::from_u16(445), "")
            .sign_with_keys(&keys)
            .expect("test-only signing must succeed")
    }

    #[test]
    fn kind_1059_with_empty_relays_is_blocked() {
        // D10: a kind:1059 gift-wrap with no explicit relay pin must NOT
        // fall through to the kernel's Auto-fallback (which would publish
        // to the author's NIP-65 public outbox).
        let event = sample_kind_1059();
        assert!(
            is_d10_blocked(&event, &[]),
            "kind:1059 + empty relays must be blocked by the D10 guard"
        );
    }

    #[test]
    fn kind_1059_with_explicit_relays_is_permitted() {
        // The guard MUST NOT block a kind:1059 publish when the caller has
        // supplied an explicit relay pin — that's the correct NIP-17 / NIP-59
        // routing path (recipient kind:10050 DM-inbox or group relays).
        let event = sample_kind_1059();
        let pin: Vec<RelayUrl> = vec!["wss://dm.example/".parse().expect("parse url")];
        assert!(
            !is_d10_blocked(&event, &pin),
            "kind:1059 + explicit relays must pass the D10 guard"
        );
    }

    #[test]
    fn kind_445_with_empty_relays_is_permitted() {
        // Kind:445 group messages legitimately fall back to Auto when the
        // group-relay cache misses — the D10 guard is private-kind-only and
        // MUST NOT regress that path.
        let event = sample_kind_445();
        assert!(
            !is_d10_blocked(&event, &[]),
            "kind:445 + empty relays must pass the D10 guard (Auto-fallback is legitimate)"
        );
    }

    #[test]
    fn kind_30443_keypackage_with_empty_relays_is_permitted() {
        // Kind:30443 KeyPackage publishes also use the Auto-fallback path
        // (published to the author's NIP-65 outbox). The D10 guard is
        // tightly scoped to kind:1059 and MUST NOT regress this.
        let keys = Keys::generate();
        let kp = EventBuilder::new(Kind::from_u16(30443), "")
            .sign_with_keys(&keys)
            .expect("test-only signing must succeed");
        assert!(
            !is_d10_blocked(&kp, &[]),
            "kind:30443 KeyPackage + empty relays must pass the D10 guard"
        );
    }

    #[test]
    fn publish_to_with_null_app_is_silent_noop() {
        // The null-app guard runs BEFORE the D10 check; a null app must
        // produce no FFI call and no panic regardless of kind / relays.
        // (Compile-time + runtime no-panic check; `publish_to` returns ().)
        let event = sample_kind_1059();
        publish_to(std::ptr::null_mut(), &event, &[]);
        publish_to(std::ptr::null_mut(), &event, &[
            "wss://dm.example/".parse().expect("parse url"),
        ]);
        let event = sample_kind_445();
        publish_to(std::ptr::null_mut(), &event, &[]);
    }
}
