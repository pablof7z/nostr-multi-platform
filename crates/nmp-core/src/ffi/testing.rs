//! Test-support FFI injectors.
//!
//! Split out of `ffi/mod.rs` to keep both files under the 300-LOC soft cap.
//! The whole module is gated on `cfg(any(test, feature = "test-support"))`;
//! these symbols are never part of the production FFI ABI (D0).  Re-exported
//! from `ffi/mod.rs` so `crate::ffi::nmp_app_inject_*` paths stay byte-stable.
//!
//! The file-level `#![cfg(...)]` below is redundant with the gated
//! `mod testing;` declaration in `ffi/mod.rs`, but kept deliberately: it makes
//! this file self-describing as test-only, so the `ci/check-ffi-header-drift.sh`
//! gate can recognise it (and any future tooling) without parsing `mod.rs`.
#![cfg(any(test, feature = "test-support"))]

use super::{app_ref, NmpApp};
use crate::actor::ActorCommand;
use std::ffi::{c_char, CStr};

/// Inject `count` pre-verified kind-1 events into the kernel timeline via
/// the test-support `ingest_pre_verified_event` path.
///
/// Events are constructed with deterministic IDs/pubkeys using
/// `VerifiedEvent::from_raw_unchecked` (test-support fast path; bypasses
/// Schnorr verification for harness ergonomics — see D0 note below).
///
/// D0: this symbol is gated on `cfg(any(test, feature = "test-support"))` and
/// is never part of the production FFI surface.  Swift/C callers never see it.
/// The `VerifiedEvent` type is the capability boundary: production code can
/// only construct one via `try_from_raw` (full Schnorr verify).  This function
/// uses `from_raw_unchecked` explicitly for legacy perf-harness compatibility.
///
/// Prefer `inject_signed_events` for new harnesses (S3/S4/S5 all use it now):
/// it produces real Schnorr-signed events via `EventBuilder::sign_with_keys`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_inject_pre_verified_events(
    app: *mut NmpApp,
    base_id_prefix: *const c_char,
    base_created_at: u64,
    count: u32,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let prefix = if base_id_prefix.is_null() {
        "stress".to_string()
    } else {
        // SAFETY: non-null pointer checked above.
        unsafe { CStr::from_ptr(base_id_prefix) }
            .to_str()
            .unwrap_or("stress")
            .to_string()
    };

    // Pool of 8 deterministic pubkeys (64 hex chars each) for the harness.
    const POOL: &[&str] = &[
        "0000000000000000000000000000000000000000000000000000000000000001",
        "0000000000000000000000000000000000000000000000000000000000000002",
        "0000000000000000000000000000000000000000000000000000000000000003",
        "0000000000000000000000000000000000000000000000000000000000000004",
        "0000000000000000000000000000000000000000000000000000000000000005",
        "0000000000000000000000000000000000000000000000000000000000000006",
        "0000000000000000000000000000000000000000000000000000000000000007",
        "0000000000000000000000000000000000000000000000000000000000000008",
    ];

    let events: Vec<crate::store::VerifiedEvent> = (0..count as u64)
        .map(|i| {
            // 64-hex event ID derived from prefix + index.
            let raw_id = format!("{prefix}{i:0>16x}");
            let id = format!("{raw_id:0<64}");
            let id = id[..64].to_string();
            let pubkey = POOL[(i as usize) % POOL.len()].to_string();
            let created_at = base_created_at.saturating_add(i);
            let content = format!("harness event {i}");
            let raw = crate::store::RawEvent {
                id,
                pubkey,
                created_at,
                kind: 1,
                tags: Vec::new(),
                content,
                // Placeholder sig — from_raw_unchecked bypasses verification.
                // D0 gate: this path is cfg-gated and excluded from the production
                // FFI ABI.  Use inject_signed_events for full Schnorr verify path.
                sig: "0".repeat(128),
            };
            crate::store::VerifiedEvent::from_raw_unchecked(raw)
        })
        .collect();

    app.send_cmd(ActorCommand::IngestPreVerifiedEvents(events));
}

/// Inject `count` real Schnorr-signed kind-1 events into the kernel timeline
/// via the full `try_from_raw` verification path.
///
/// Uses `nostr::Keys::generate() + EventBuilder::text_note + sign_with_keys`
/// to produce cryptographically valid events.  Schnorr sign cost is ~30–50 µs
/// per event; for S4 (500 events) and S5 (200 events) this is 10–25 ms total.
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`.  Not part of the
/// production FFI ABI.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_inject_signed_events(
    app: *mut NmpApp,
    base_created_at: u64,
    count: u32,
) {
    use nostr::{EventBuilder, Keys, Timestamp};

    let Some(app) = app_ref(app) else {
        return;
    };

    // Single fixture key: generate once, sign all events.
    let keys = Keys::generate();
    let events: Vec<crate::store::VerifiedEvent> = (0..count as u64)
        .filter_map(|i| {
            let ts = Timestamp::from(base_created_at.saturating_add(i));
            let nostr_event = EventBuilder::text_note(format!("signed harness event {i}"))
                .custom_created_at(ts)
                .sign_with_keys(&keys)
                .ok()?;
            let raw = crate::store::RawEvent {
                id: nostr_event.id.to_hex(),
                pubkey: nostr_event.pubkey.to_hex(),
                created_at: nostr_event.created_at.as_secs(),
                kind: nostr_event.kind.as_u16() as u32,
                tags: nostr_event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: nostr_event.content.clone(),
                sig: nostr_event.sig.to_string(),
            };
            // try_from_raw: full Schnorr + id-hash verification.
            crate::store::VerifiedEvent::try_from_raw(raw).ok()
        })
        .collect();

    app.send_cmd(ActorCommand::IngestPreVerifiedEvents(events));
}
