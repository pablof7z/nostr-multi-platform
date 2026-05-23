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
use std::ffi::{c_char, CStr, CString};

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

/// Inject a single real signed event (supplied as NIP-01 JSON) through the
/// kernel's `IngestPreVerifiedEvents` path.
///
/// The JSON string is parsed and passed through full Schnorr + id-hash
/// verification via `try_from_raw`.  The event then routes through
/// `ingest_pre_verified_event`, which calls both `notify_event_observers` AND
/// `notify_raw_event_observers` on `Inserted|Replaced` outcomes (test-seam fix).
///
/// This unblocks integration tests that need to inject a real signed event (e.g.
/// a kind:1059 gift-wrap from `nmp_nip59::gift_wrap`) through the kernel so
/// registered `RawEventObserver`s (e.g. `DmInboxProjection`) see it exactly as
/// production relay delivery would.
///
/// Returns `true` on success, `false` if the JSON is malformed or Schnorr
/// verification fails — callers should assert the return value in tests.
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`. Never part of the
/// production FFI ABI.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_inject_signed_event_json(
    app: *mut NmpApp,
    event_json: *const c_char,
) -> bool {
    use nostr::JsonUtil;

    let Some(app) = app_ref(app) else {
        return false;
    };
    if event_json.is_null() {
        return false;
    }
    // SAFETY: non-null pointer checked above; caller guarantees the lifetime.
    let json_str = match unsafe { CStr::from_ptr(event_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let nostr_event = match nostr::Event::from_json(json_str) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let raw = crate::store::RawEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    };
    // Full Schnorr + id-hash verification — real events only.
    let verified = match crate::store::VerifiedEvent::try_from_raw(raw) {
        Ok(v) => v,
        Err(_) => return false,
    };
    app.send_cmd(ActorCommand::IngestPreVerifiedEvents(vec![verified]));
    true
}

/// Read a single snapshot-projection's JSON by key, returning a heap-owned
/// C string the caller must free via [`crate::ffi::nmp_app_free_string`].
///
/// Runs every registered snapshot projection directly against the app's
/// shared registry (the same path `make_update` drives on each actor tick),
/// then pulls out the value at `key`. Returns `null` when:
///
/// * `app` or `key` is null,
/// * `key` is not valid UTF-8,
/// * no projection has been registered under `key`,
/// * serialization of the projection's `serde_json::Value` fails (shouldn't
///   happen for any well-formed projection), or
/// * the resulting `CString` would contain an interior NUL.
///
/// The returned pointer is heap-owned (`CString::into_raw`); failing to free
/// it via `nmp_app_free_string` leaks the underlying allocation.
///
/// This is the symmetric output-side seam for `nmp_app_inject_signed_event_json`:
/// together they let an integration test inject a verbatim signed event and
/// observe its effect through the registered snapshot-projection layer, with
/// no production code paths exposed beyond what the kernel already runs on
/// every snapshot tick.
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`. Never part of the
/// production FFI ABI.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_read_projection_json(
    app: *mut NmpApp,
    key: *const c_char,
) -> *mut c_char {
    let Some(app) = app_ref(app) else {
        return std::ptr::null_mut();
    };
    if key.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: non-null pointer checked above; caller guarantees the lifetime.
    let key_str = match unsafe { CStr::from_ptr(key) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    // Lock the shared snapshot-projection slot directly and run every closure.
    // Mirrors the `pub(crate) fn run_snapshot_projections_for_test` helper at
    // `ffi/mod.rs:831` (which is `#[cfg(test)]`-gated and not visible under
    // the `test-support` feature). Reaching the field directly is fine: this
    // module is a sibling of `mod.rs` inside `crate::ffi`, so the private
    // field is in-scope without widening any visibility.
    let projections = match app.snapshot_projections.lock() {
        Ok(registry) => registry.run(),
        Err(_) => return std::ptr::null_mut(),
    };
    let Some(value) = projections.get(key_str) else {
        return std::ptr::null_mut();
    };
    let Ok(json) = serde_json::to_string(value) else {
        return std::ptr::null_mut();
    };
    match CString::new(json) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
