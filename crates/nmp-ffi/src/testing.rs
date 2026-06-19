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
use nmp_core::ActorCommand;
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

    let events: Vec<nmp_store::VerifiedEvent> = (0..count as u64)
        .map(|i| {
            // 64-hex event ID derived from prefix + index.
            let raw_id = format!("{prefix}{i:0>16x}");
            let id = format!("{raw_id:0<64}");
            let id = id[..64].to_string();
            let pubkey = POOL[(i as usize) % POOL.len()].to_string();
            let created_at = base_created_at.saturating_add(i);
            let content = format!("harness event {i}");
            let raw = nmp_store::RawEvent {
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
            nmp_store::VerifiedEvent::from_raw_unchecked(raw)
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
pub extern "C" fn nmp_app_inject_signed_events(app: *mut NmpApp, base_created_at: u64, count: u32) {
    use nostr::{EventBuilder, Keys, Timestamp};

    let Some(app) = app_ref(app) else {
        return;
    };

    // Single fixture key: generate once, sign all events.
    let keys = Keys::generate();
    let events: Vec<nmp_store::VerifiedEvent> = (0..count as u64)
        .filter_map(|i| {
            let ts = Timestamp::from(base_created_at.saturating_add(i));
            let nostr_event = EventBuilder::text_note(format!("signed harness event {i}"))
                .custom_created_at(ts)
                .sign_with_keys(&keys)
                .ok()?;
            let raw = nmp_store::RawEvent {
                id: nostr_event.id.to_hex(),
                pubkey: nostr_event.pubkey.to_hex(),
                created_at: nostr_event.created_at.as_secs(),
                kind: nostr_event.kind.as_u16() as u32,
                tags: nostr_event
                    .tags
                    .iter()
                    .map(|t| t.as_slice().to_vec())
                    .collect(),
                content: nostr_event.content.clone(),
                sig: nostr_event.sig.to_string(),
            };
            // try_from_raw: full Schnorr + id-hash verification.
            nmp_store::VerifiedEvent::try_from_raw(raw).ok()
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
/// registered `IngestParser`s (e.g. `DmInboxProjection`) see it exactly as
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
    let raw = nmp_store::RawEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event
            .tags
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    };
    // Full Schnorr + id-hash verification — real events only.
    let verified = match nmp_store::VerifiedEvent::try_from_raw(raw) {
        Ok(v) => v,
        Err(_) => return false,
    };
    app.send_cmd(ActorCommand::IngestPreVerifiedEvents(vec![verified]));
    true
}

/// Test-support — configure a durable LRU budget ceiling before `nmp_app_start`.
///
/// Sets the `max_total_events` ceiling used by `Kernel::derive_store_gc_inputs`
/// when the kernel runs its 60-second GC pass.  Must be called before
/// `nmp_app_start`; calls after start are a no-op (returns
/// `NmpConfigStatus::AlreadyStarted`).
///
/// Production default (`usize::MAX`, LRU disabled) is never touched — only this
/// test-support path enables bounded LRU eviction.
///
/// Return values mirror `NmpConfigStatus`:
///  - 0 = Ok
///  - 1 = NullApp
///  - 2 = AlreadyStarted
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`. Not part of the
/// production FFI ABI.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_configure_gc_budget(app: *mut NmpApp, max_events: u64) -> u32 {
    use super::prestart_config::NmpConfigStatus;
    let Some(app) = app_ref(app) else {
        return NmpConfigStatus::NullApp.code();
    };
    if let Err(status) = app.ensure_prestart_config(
        "gc_budget",
        "gc_budget_ceiling",
        "nmp_app_configure_gc_budget",
    ) {
        return status.code();
    }
    if let Ok(mut guard) = app.gc_budget_ceiling.lock() {
        *guard = Some(max_events as usize);
    }
    NmpConfigStatus::Ok.code()
}

/// Test-support — ingest `count` real Schnorr-signed kind:1 events under an
/// UN-PINNED sub-id, then BLOCK until the actor finishes the batch.
///
/// Unlike `nmp_app_inject_signed_event_json` (which routes through the
/// `"diag-firehose-stress"` sub-id and pins every event into `self.timeline`),
/// this routes through `ActorCommand::IngestPreVerifiedEventsForSubId` under the
/// `"gc-oracle-unpinned"` sub-id. That sub-id does NOT start with
/// `diag-firehose-`, so `ingest_pre_verified_event` skips the
/// `self.timeline.push_back`: the injected events land in the RAM cache and the
/// durable store but are NOT timeline-pinned — they are eviction-eligible, which
/// is exactly what a GC oracle needs to observe a real eviction.
///
/// The events are signed with a freshly generated key (NOT the caller's author),
/// so an author-scoped store scan of the caller's own key returns only the
/// caller's pinned events, never this filler corpus.
///
/// Returns the number of events accepted (signed + verified + enqueued). Blocks
/// on the ingest ack so the corpus is SETTLED on return — no fixed sleep.
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`. Not part of the
/// production FFI ABI.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_inject_unpinned_events_for_gc(
    app: *mut NmpApp,
    base_created_at: u64,
    count: u32,
) -> u32 {
    use nostr::{EventBuilder, Keys, Timestamp};

    let Some(app) = app_ref(app) else {
        return 0;
    };

    // Separate fixture key: this filler corpus is intentionally NOT authored by
    // the caller, so a caller-author store scan can isolate pinned survivors.
    let keys = Keys::generate();
    let events: Vec<nmp_store::VerifiedEvent> = (0..count as u64)
        .filter_map(|i| {
            let ts = Timestamp::from(base_created_at.saturating_add(i));
            let nostr_event = EventBuilder::text_note(format!("gc-unpinned filler {i}"))
                .custom_created_at(ts)
                .sign_with_keys(&keys)
                .ok()?;
            let raw = nmp_store::RawEvent {
                id: nostr_event.id.to_hex(),
                pubkey: nostr_event.pubkey.to_hex(),
                created_at: nostr_event.created_at.as_secs(),
                kind: nostr_event.kind.as_u16() as u32,
                tags: nostr_event
                    .tags
                    .iter()
                    .map(|t| t.as_slice().to_vec())
                    .collect(),
                content: nostr_event.content.clone(),
                sig: nostr_event.sig.to_string(),
            };
            nmp_store::VerifiedEvent::try_from_raw(raw).ok()
        })
        .collect();
    let accepted = events.len() as u32;

    let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(1);
    app.send_cmd(ActorCommand::IngestPreVerifiedEventsForSubId {
        sub_id: "gc-oracle-unpinned".to_string(),
        events,
        ack: ack_tx,
    });
    // Block until the actor has ingested + re-sorted the whole batch (settled).
    let _ = ack_rx.recv_timeout(std::time::Duration::from_secs(30));
    accepted
}

/// Test-support — force one immediate GC pass outside the 60-second tick, then
/// BLOCK until the pass has fully completed.
///
/// Sends `ActorCommand::TriggerGcStep { ack }` to the kernel actor, which runs
/// `Kernel::run_gc_step()` (RAM-tier eviction + store-tier LRU step) and then
/// acks. This call blocks on that ack, so on return the cumulative eviction
/// counters read by `nmp_app_read_ram_eviction_stats` reflect a SETTLED GC pass
/// — no `std::thread::sleep` + polling guesswork.
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`. Not part of the
/// production FFI ABI.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_trigger_gc_step(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(1);
    app.send_cmd(ActorCommand::TriggerGcStep { ack: ack_tx });
    // Block until the GC pass is settled (RAM eviction + store LRU step done).
    let _ = ack_rx.recv_timeout(std::time::Duration::from_secs(30));
}

/// Test-support — read cumulative RAM and durable-store LRU eviction counters.
///
/// Both counters start at 0 at process start and only increase.  Take a
/// snapshot before and after a measurement window to compute per-window deltas.
///
/// - `out_ram_evicted`  → cumulative count of events evicted from the kernel's
///   RAM-tier `self.events` cache (`Kernel::evict_events_cache`).
/// - `out_lru_evicted`  → cumulative count of events LRU-deleted from the
///   durable store (`Kernel::run_gc_step` → `gc_step_with_pins_and_coverage`).
///   Always 0 when using the in-memory store (no LMDB configured).
///
/// Either output pointer may be null (the corresponding counter is skipped).
/// Thread-safe (backed by `AtomicU64 + Relaxed` loads).
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`. Not part of the
/// production FFI ABI.
#[no_mangle]
pub extern "C" fn nmp_app_read_ram_eviction_stats(
    out_ram_evicted: *mut u64,
    out_lru_evicted: *mut u64,
) {
    use std::sync::atomic::Ordering;
    if !out_ram_evicted.is_null() {
        // SAFETY: non-null pointer checked above; caller guarantees the lifetime.
        unsafe {
            *out_ram_evicted =
                nmp_core::testing::PROCESS_RAM_EVENTS_EVICTED.load(Ordering::Relaxed);
        }
    }
    if !out_lru_evicted.is_null() {
        // SAFETY: non-null pointer checked above; caller guarantees the lifetime.
        unsafe {
            *out_lru_evicted =
                nmp_core::testing::PROCESS_STORE_LRU_EVICTED.load(Ordering::Relaxed);
        }
    }
}

/// Test-support — return the event IDs (and author pubkey) of stored kind:1
/// events for the given author, as a JSON array string.
///
/// Scans the event store (in-memory or LMDB depending on configuration) for
/// kind:1 events authored by `pubkey_hex`, newest-first, up to `limit` events.
/// Returns a heap-allocated NUL-terminated JSON string `[{"id":"…","author":"…"},…]`,
/// or `null` on error (null app pointer, empty pubkey, store not yet published).
/// The caller MUST free the string via `nmp_free_string`.
///
/// This is Seam 1 (`nmp_app_read_feed_authors`): it exposes the set of event
/// ids the store holds for a given author so the reactive oracle can assert
/// `corpus_ids ⊆ stored_ids` without scraping the churning RAM cache.
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`. Not part of the
/// production FFI ABI.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_read_author_event_ids(
    app: *mut NmpApp,
    pubkey_hex: *const c_char,
    limit: u32,
) -> *mut c_char {
    use std::ffi::CString;

    let Some(app) = app_ref(app) else {
        return std::ptr::null_mut();
    };
    if pubkey_hex.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: non-null pointer checked above; caller guarantees the lifetime.
    let author_hex = match unsafe { CStr::from_ptr(pubkey_hex) }.to_str() {
        Ok(s) if !s.is_empty() => s,
        _ => return std::ptr::null_mut(),
    };

    // Parse hex pubkey → 32-byte `PubKey` (the store key type).
    let pubkey_bytes: nmp_store::PubKey = match nostr::PublicKey::from_hex(author_hex) {
        Ok(pk) => {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(pk.as_bytes());
            bytes
        }
        Err(_) => return std::ptr::null_mut(),
    };

    // Borrow the event store via the publish-back slot.
    let store = match app.event_store_handle().lock() {
        Ok(guard) => match guard.as_ref().map(|s| std::sync::Arc::clone(s)) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        },
        Err(_) => return std::ptr::null_mut(),
    };

    let scan_limit = if limit == 0 { 2000 } else { limit as usize };
    let iter = match store.scan_by_author_kind(
        &pubkey_bytes,
        &[1u32],
        None,
        None,
        scan_limit,
    ) {
        Ok(it) => it,
        Err(_) => return std::ptr::null_mut(),
    };

    // Collect results into a JSON array of {"id":"…","author":"…"} objects.
    let mut entries: Vec<String> = Vec::new();
    for result in iter {
        if let Ok(ev) = result {
            entries.push(format!(
                r#"{{"id":"{}","author":"{}"}}"#,
                ev.raw.id, ev.raw.pubkey
            ));
        }
    }
    let json = format!("[{}]", entries.join(","));
    CString::new(json)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// ADR-0055 Rung 0 — read cumulative per-projection churn counters.
///
/// Returns the process-lifetime totals of typed projections serialized and
/// changed (content differed from the prior tick) across ALL `make_update`
/// ticks since process start. The caller takes a snapshot before and after a
/// measurement window and computes the delta to get per-window figures.
///
/// Both counters start at 0 at process start and never decrease.
/// Thread-safe (backed by `AtomicU64 + Relaxed` loads).
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`. Never part of the
/// production FFI ABI.
#[no_mangle]
pub extern "C" fn nmp_app_read_projection_churn_stats(
    out_serialized: *mut u64,
    out_changed: *mut u64,
) {
    use std::sync::atomic::Ordering;
    if !out_serialized.is_null() {
        // SAFETY: non-null pointer checked above; caller guarantees the lifetime.
        unsafe {
            *out_serialized =
                nmp_core::testing::PROCESS_PROJECTIONS_SERIALIZED.load(Ordering::Relaxed);
        }
    }
    if !out_changed.is_null() {
        // SAFETY: non-null pointer checked above; caller guarantees the lifetime.
        unsafe {
            *out_changed =
                nmp_core::testing::PROCESS_PROJECTIONS_CHANGED.load(Ordering::Relaxed);
        }
    }
}

