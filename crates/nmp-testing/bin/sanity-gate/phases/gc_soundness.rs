//! Robustness family 5 — GC SOUNDNESS (deeper than "RSS is flat").
//!
//! A prior version of this oracle measured the WRONG thing. It injected its
//! corpus through `nmp_app_inject_signed_event_json`, which routes every event
//! under the `"diag-firehose-stress"` sub-id — and that sub-id PINS each event
//! into `self.timeline`. Timeline membership protects an event from BOTH RAM-
//! cache eviction (`evict_events_cache`) and durable-LRU eviction
//! (`derive_store_pin_set`). So the corpus was never eviction-eligible:
//! `ram_delta == 0` / `lru_delta == 0` were the EXPECTED, correct product
//! behaviour, yet the oracle read them as a pass-by-luck / vacuous gate. The GC
//! product path was correct all along — it simply was never exercised.
//!
//! This rewrite fixes the ORACLE:
//!
//! 1. UN-PINNED corpus. `nmp_app_inject_unpinned_events_for_gc` routes the
//!    filler corpus through `ActorCommand::IngestPreVerifiedEventsForSubId`
//!    under the `"gc-oracle-unpinned"` sub-id, which does NOT push into
//!    `self.timeline`. The events are therefore eviction-eligible in both tiers.
//! 2. Acked barriers (no fixed sleep). Both the un-pinned ingest and
//!    `nmp_app_trigger_gc_step` block on a one-shot actor ack, so every counter
//!    is read against a SETTLED state.
//! 3. Three real gates against one settled GC pass:
//!    - `gc-ram-cache-eviction`: assert `ram_delta > 0` (RAM hot-cache eviction
//!      fired once the cache exceeded `EVENTS_RAM_HWM = 1000`).
//!    - `gc-durable-lru-opt-in`: with a finite `GcBudget` ceiling of 50, assert
//!      `lru_delta > 0` (the `MemEventStore` Phase-2 durable LRU deleted rows).
//!    - `gc-no-stranded-coverage`: FIRST prove an eviction occurred, THEN assert
//!      every PINNED (timeline-pinned, oldest-`created_at`) event survived in the
//!      store: `pinned_ids ⊆ stored_readable_ids`. A stranded pinned event is a
//!      real FAIL, surfaced as a finding.

use std::collections::HashSet;

use nostr::{EventBuilder, JsonUtil, Keys, ToBech32};

use nmp_ffi::{
    nmp_app_configure_gc_budget, nmp_app_inject_signed_event_json,
    nmp_app_inject_unpinned_events_for_gc, nmp_app_read_author_event_ids,
    nmp_app_read_ram_eviction_stats, nmp_app_trigger_gc_step, nmp_free_string,
};

use crate::config::{Args, Phase};
use crate::report::{GateRow, SanityReport, Verdict};

/// Number of UN-PINNED filler events (must exceed `EVENTS_RAM_HWM = 1000` so RAM
/// eviction fires, and exceed the durable ceiling so LRU eviction fires).
const GC_UNPINNED_BATCH: u32 = 1_200;
/// Number of PINNED (timeline) events — the oldest `created_at`, so they are the
/// first durable-LRU candidates and would be evicted but for the pin.
const GC_PINNED_COUNT: u64 = 10;
/// RAM HWM from `nmp-core` (`EVENTS_RAM_HWM`) — named here for the note text.
const EVENTS_RAM_HWM: u64 = 1_000;
/// Finite durable LRU ceiling, well below the corpus size, to opt into Phase-2
/// store eviction (`GcBudget::with_durable_event_ceiling`).
const GC_BUDGET_CEILING: u64 = 50;

pub fn run_gc_soundness(report: &mut SanityReport, _args: &Args) {
    use nmp_app_chirp::{
        nmp_app_chirp_declare_consumed_projections, nmp_app_chirp_register,
        nmp_app_chirp_unregister, ChirpHandle,
    };
    use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_app_signin_nsec, nmp_app_start};
    use std::ffi::CString;

    let phase = Phase::GcSoundness.as_str();

    // Oracle keypair: every PINNED event is self-authored so the author-scoped
    // store scan isolates pinned survivors (the un-pinned filler uses a separate
    // key inside the FFI, never this author).
    let keys = Keys::generate();
    let nsec = keys.secret_key().to_bech32().unwrap_or_default();
    let pubkey_hex = keys.public_key().to_hex();

    let app = nmp_app_new();

    let pk_c = CString::new(pubkey_hex.as_str()).unwrap();
    let mut chirp: *mut ChirpHandle = std::ptr::null_mut();
    nmp_app_chirp_register(app, pk_c.as_ptr(), &mut chirp);
    nmp_app_chirp_declare_consumed_projections(app);

    if let Ok(nsec_c) = CString::new(nsec.as_str()) {
        nmp_app_signin_nsec(app, nsec_c.as_ptr(), 1);
    }

    // ── Opt into a finite durable LRU ceiling BEFORE start ──────────────────
    let configure_status = nmp_app_configure_gc_budget(app, GC_BUDGET_CEILING);
    if configure_status != 0 {
        for gate in [
            "gc-ram-cache-eviction",
            "gc-durable-lru-opt-in",
            "gc-no-stranded-coverage",
        ] {
            report.push(GateRow::unmeasured(
                gate,
                phase,
                "nmp_app_configure_gc_budget",
                "pre-start GC budget ceiling",
                "configure_status == 0 (Ok)",
                Verdict::Blocked,
                &format!(
                    "nmp_app_configure_gc_budget returned status={configure_status} (expected 0); \
                     cannot exercise GC — pre-start guard rejected the call"
                ),
            ));
        }
        if !chirp.is_null() {
            nmp_app_chirp_unregister(chirp);
        }
        nmp_app_free(app);
        return;
    }

    nmp_app_start(app, 0, 500, 4);

    // ── Eviction counters BEFORE inject ─────────────────────────────────────
    let mut ram_before: u64 = 0;
    let mut lru_before: u64 = 0;
    nmp_app_read_ram_eviction_stats(&mut ram_before, &mut lru_before);

    // ── Inject the PINNED corpus (oldest timestamps, timeline-pinned) ───────
    // These flow through `nmp_app_inject_signed_event_json` → the
    // `diag-firehose-stress` sub-id → `self.timeline.push_back`, so they are
    // pinned in BOTH tiers. Oldest `created_at` ⇒ first LRU candidates.
    let base_ts = crate::report::now_unix();
    let mut pinned_ids: Vec<String> = Vec::new();
    for i in 0..GC_PINNED_COUNT {
        let ts = nostr::Timestamp::from(base_ts + i);
        if let Ok(ev) = EventBuilder::text_note(format!("gc-oracle pinned {i}"))
            .custom_created_at(ts)
            .sign_with_keys(&keys)
        {
            let ev: nostr::Event = ev;
            let id_hex = ev.id.to_hex();
            if let Ok(c) = CString::new(ev.as_json()) {
                if nmp_app_inject_signed_event_json(app, c.as_ptr()) {
                    pinned_ids.push(id_hex);
                }
            }
        }
    }

    // ── Inject the UN-PINNED filler (newer timestamps) — acked barrier ──────
    // Routed under `gc-oracle-unpinned`, so NOT timeline-pinned ⇒ evictable.
    let unpinned_base_ts = base_ts + 1_000;
    let unpinned_accepted =
        nmp_app_inject_unpinned_events_for_gc(app, unpinned_base_ts, GC_UNPINNED_BATCH);

    // ── Force a settled GC pass — acked barrier (no sleep) ──────────────────
    nmp_app_trigger_gc_step(app);

    // ── Eviction counters AFTER GC ──────────────────────────────────────────
    let mut ram_after: u64 = 0;
    let mut lru_after: u64 = 0;
    nmp_app_read_ram_eviction_stats(&mut ram_after, &mut lru_after);

    let ram_delta = ram_after.saturating_sub(ram_before);
    let lru_delta = lru_after.saturating_sub(lru_before);

    // ── Read the post-GC store survivors for the oracle author ──────────────
    let stored_ids: HashSet<String> = {
        let pk = CString::new(pubkey_hex.as_str()).ok();
        pk.map(|pk| nmp_app_read_author_event_ids(app, pk.as_ptr(), 0))
            .and_then(|ptr| {
                if ptr.is_null() {
                    return None;
                }
                let parsed = unsafe { std::ffi::CStr::from_ptr(ptr) }
                    .to_str()
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
                nmp_free_string(ptr);
                parsed
            })
            .and_then(|v| {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|e| e.get("id").and_then(|i| i.as_str()).map(String::from))
                        .collect()
                })
            })
            .unwrap_or_default()
    };

    // ── Gate 1: RAM hot-cache eviction fired ────────────────────────────────
    report.push(
        GateRow::min(
            "gc-ram-cache-eviction",
            phase,
            "nmp_app_inject_unpinned_events_for_gc + nmp_app_trigger_gc_step (acked)",
            "PROCESS_RAM_EVENTS_EVICTED delta after settled GC",
            ram_delta as f64,
            1.0,
            "ram-evicted",
        )
        .with_note(&format!(
            "unpinned_injected={unpinned_accepted}; pinned_injected={}; \
             ram_cache_hwm={EVENTS_RAM_HWM}; ram_delta={ram_delta}; \
             un-pinned corpus made the RAM cache exceed the HWM so eviction could fire",
            pinned_ids.len()
        )),
    );

    // ── Gate 2: durable LRU eviction fired (finite ceiling) ─────────────────
    report.push(
        GateRow::min(
            "gc-durable-lru-opt-in",
            phase,
            "nmp_app_configure_gc_budget(50) + nmp_app_trigger_gc_step (acked)",
            "PROCESS_STORE_LRU_EVICTED delta (GcReport.lru_evicted) after settled GC",
            lru_delta as f64,
            1.0,
            "lru-evicted",
        )
        .with_note(&format!(
            "durable_ceiling={GC_BUDGET_CEILING}; corpus={}+{unpinned_accepted}; \
             lru_delta={lru_delta}; MemEventStore performs Phase-2 LRU deletion when \
             max_total_events is finite (mem/gc.rs) — NOT excused as in-memory",
            pinned_ids.len()
        )),
    );

    // ── Gate 3: no stranding — prove eviction, then prove pins survived ─────
    let eviction_occurred = ram_delta + lru_delta > 0;
    let present: Vec<&String> = pinned_ids
        .iter()
        .filter(|id| stored_ids.contains(*id))
        .collect();
    let stranded: Vec<&String> = pinned_ids
        .iter()
        .filter(|id| !stored_ids.contains(*id))
        .collect();
    let all_pinned_present = stranded.is_empty() && !pinned_ids.is_empty();

    let verdict = if eviction_occurred && all_pinned_present {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    if !stranded.is_empty() {
        report.finding(format!(
            "GC SOUNDNESS FAIL: {} pinned event(s) STRANDED (evicted despite the pin): {:?}",
            stranded.len(),
            stranded
        ));
    }
    if !eviction_occurred {
        report.finding(
            "GC SOUNDNESS: no eviction fired (ram_delta=0 AND lru_delta=0) after an \
             un-pinned corpus + acked GC — real product finding, the no-stranding \
             invariant is unproven (vacuous)."
                .to_string(),
        );
    }
    report.push(GateRow {
        gate: "gc-no-stranded-coverage".to_string(),
        phase: phase.to_string(),
        tool: "nmp_app_read_author_event_ids after settled GC".to_string(),
        hook: "pinned_ids ⊆ stored_readable_ids (after a proven eviction)".to_string(),
        threshold: "eviction>0 AND all pinned readable".to_string(),
        measured: Some(format!(
            "{}/{} pinned readable; evicted={} (ram={ram_delta},lru={lru_delta})",
            present.len(),
            pinned_ids.len(),
            ram_delta + lru_delta
        )),
        verdict,
        note: Some(format!(
            "store_survivors_for_author={}; pinned timeline events have the OLDEST \
             created_at (first LRU candidates) yet must survive durable eviction; \
             stranded={:?}",
            stored_ids.len(),
            stranded
        )),
    });

    if !chirp.is_null() {
        nmp_app_chirp_unregister(chirp);
    }
    nmp_app_free(app);
}
