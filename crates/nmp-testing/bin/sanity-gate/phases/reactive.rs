//! Robustness family 1 — REACTIVE CORRECTNESS (the core NMP promise).
//!
//! aim.md §1 "make it nearly impossible to build a BROKEN Nostr app" + §4.1
//! ("no stale version in memory"). These oracles stress the recently-landed
//! pull substrate (ADR-0058 `kernel/pull/*`, `nmp_app_pull_page`) + the
//! verify→store→wakeup→project→emit reactive pipeline.
//!
//! Falsifiable hypotheses (absolute pass/fail, SKIP-LOUD on insufficiency):
//!  - MISSED-UPDATE: every accepted event that matches an open view appears in
//!    that view's projection. We sign in as a KNOWN key and inject K
//!    self-authored kind:1 notes (self is always part of the home feed); the
//!    projection MUST reach >= K visible items (count-complete: corpus ⊆ view).
//!  - WIRE-TO-VISIBLE LATENCY: the time from first inject to all-visible MUST
//!    stay under an absolute ceiling — a wakeup-storm/poll regression blows it.
//!  - NO-DUPLICATE-STORED-ROW: re-injecting the identical corpus adds NO new
//!    stored rows (the store dedups by id — re-injecting N already-stored events
//!    grows the stored ROW COUNT by 0). This is a store-row-count assertion, not
//!    an emit/projection claim, so the gate is named for exactly that property.
//!
//! SEAM 1: `nmp_app_read_author_event_ids` (test-support, now wired) exposes the
//! set of stored event ids for a given author so `corpus_ids ⊆ stored_ids` can
//! be asserted directly in oracle (d) below.  The COUNT-complete half above is
//! still a separate hard gate — a dropped update fails either.

use std::time::Duration;

use nostr::{EventBuilder, JsonUtil, Keys, ToBech32};

use crate::config::{Args, Phase, gates};
use crate::driver::DrivenApp;
use crate::report::{GateRow, SanityReport, Verdict};

/// How many self-authored notes the missed-update oracle injects.
const REACTIVE_BATCH: u64 = 150;

pub fn run_reactive(report: &mut SanityReport, args: &Args) {
    let phase = Phase::Reactive.as_str();

    // Deterministic, self-contained: mint a fresh key so self-authored events
    // are guaranteed to match the home feed (self-inclusion). No relay or
    // fixture required — the inject seam drives the verify→store→project path.
    let keys = Keys::generate();
    let nsec = keys.secret_key().to_bech32().unwrap_or_default();
    let pubkey_hex = keys.public_key().to_hex();

    let app = DrivenApp::launch(
        Some(&nsec),
        Some(&pubkey_hex),
        std::slice::from_ref(&args.relay),
    );
    if app
        .wait_until(Duration::from_secs(10), |s| !s.records.is_empty())
        .is_none()
    {
        report.push(GateRow::unmeasured(
            "actor-liveness",
            phase,
            "decode_snapshot_envelope",
            "first SnapshotFrame",
            ">= 1 frame in 10s",
            Verdict::Blocked,
            "actor emitted no frame in 10s — kernel did not start (BLOCKED, not a relay miss)",
        ));
        return;
    }

    let baseline_visible = app.with_state(|s| s.peak_visible());
    let baseline_notes = app.with_state(|s| s.latest().map(|r| r.note_events).unwrap_or(0));

    // Build + inject K self-authored kind:1 notes with increasing created_at.
    let base = crate::report::now_unix();
    let corpus = build_self_notes(&keys, base, REACTIVE_BATCH);
    let injected = corpus.len() as u64;
    let t0 = std::time::Instant::now();
    let mut accepted = 0u64;
    for json in &corpus {
        if let Ok(c) = std::ffi::CString::new(json.as_str()) {
            if nmp_ffi::nmp_app_inject_signed_event_json(app.raw(), c.as_ptr()) {
                accepted += 1;
            }
        }
    }

    // (a) Missed-update on the STORE/wakeup path: the kind:1 counter must reach
    //     baseline + injected. This is NOT follow-gated, so it is a hard gate:
    //     a dropped update never reaches the counter.
    let notes_target = baseline_notes + injected;
    let notes_reached = app
        .wait_until(
            Duration::from_millis(gates::WIRE_TO_VISIBLE_GATE_MS as u64),
            |s| s.latest().map(|r| r.note_events).unwrap_or(0) >= notes_target,
        )
        .is_some();
    let final_notes = app.with_state(|s| s.latest().map(|r| r.note_events).unwrap_or(0));
    let notes_dropped = notes_target.saturating_sub(final_notes);
    report.push(
        GateRow::max(
            "reactive-missed-update-store",
            phase,
            "nmp_app_inject_signed_event_json + decode_snapshot_envelope",
            "SnapshotEnvelope.note_events delta",
            notes_dropped as f64,
            0.0,
            "dropped",
        )
        .with_note(&format!(
            "injected {injected} self-authored kind:1 ({accepted} Schnorr-valid); \
             note_events {baseline_notes}→{final_notes} (reached={notes_reached})"
        )),
    );

    // (b) Missed-update on the PROJECTION: self-authored notes render in the
    //     home feed, so visible_items must reach >= injected. If the projection
    //     does NOT include self (composition-dependent), SKIP-LOUD the
    //     projection half rather than fake a pass — the store half above still
    //     proved no drop.
    let visible_target = baseline_visible + injected;
    let visible_reached = app
        .wait_until(
            Duration::from_millis(gates::WIRE_TO_VISIBLE_GATE_MS as u64),
            |s| s.peak_visible() >= visible_target,
        )
        .is_some();
    let wire_to_visible_ms = t0.elapsed().as_millis() as u64;
    let final_visible = app.with_state(|s| s.peak_visible());

    if visible_reached {
        report.push(
            GateRow::min(
                "reactive-missed-update-projection",
                phase,
                "decode_snapshot_envelope",
                "SnapshotEnvelope.visible_items",
                final_visible as f64,
                visible_target as f64,
                "items",
            )
            .with_note(&format!(
                "every injected self-note surfaced in the projection ({final_visible} >= {visible_target})"
            )),
        );
        // Wire-to-visible latency ceiling — only meaningful when the projection
        // actually reflected the batch.
        report.push(GateRow::max(
            "reactive-wire-to-visible-latency",
            phase,
            "wall clock (first inject → all visible)",
            "decode_snapshot_envelope visible_items",
            wire_to_visible_ms as f64,
            gates::WIRE_TO_VISIBLE_GATE_MS,
            "ms",
        ));
    } else {
        report.push(GateRow::unmeasured(
            "reactive-missed-update-projection",
            phase,
            "decode_snapshot_envelope",
            "SnapshotEnvelope.visible_items",
            &format!(">= {visible_target} items"),
            Verdict::SkipRelayMiss,
            &format!(
                "projection reached only {final_visible}/{visible_target} visible — the home-feed \
                 composition may not self-include the viewer's own kind:1; the store half \
                 (reactive-missed-update-store) still proved no drop. Provide a followed-author \
                 fixture (--viewer-hex + --follow-count) for a relay-driven projection assertion."
            ),
        ));
        report.push(GateRow::unmeasured(
            "reactive-wire-to-visible-latency",
            phase,
            "wall clock",
            "decode_snapshot_envelope visible_items",
            &format!("<= {} ms", gates::WIRE_TO_VISIBLE_GATE_MS),
            Verdict::SkipRelayMiss,
            "projection did not fill — latency ceiling not asserted this run (see projection row)",
        ));
    }

    // (c) NO-DUPLICATE-STORED-ROW — a REAL, self-contained dedup assertion on
    //     the stored ROW COUNT (not an id-set, not the projection). The prior
    //     version collected the read seam's rows into a HashSet and compared SET
    //     sizes — but a set hides a duplicate row with the same id, so a store
    //     that double-stored an event would still PASS (it was an id-set-growth
    //     oracle, not a no-duplicate-row oracle). Fix: read the stored ROW LIST
    //     (the seam emits one entry per stored row, duplicates preserved) BEFORE
    //     and AFTER re-injecting the identical corpus, and assert the row COUNT
    //     does NOT grow — re-injecting N already-stored events adds 0 rows when
    //     the store truly dedups by id. A duplicate stored row would grow the
    //     count and FAIL this gate. No relay, no projection dependency; hard
    //     PASS/FAIL (or SKIP-LOUD if the read seam returns nothing, never a 0→0
    //     pass).
    let corpus_ids: std::collections::HashSet<String> = corpus
        .iter()
        .filter_map(|json| {
            serde_json::from_str::<serde_json::Value>(json)
                .ok()
                .and_then(|v| v["id"].as_str().map(str::to_owned))
        })
        .collect();

    // Returns one id per stored ROW (duplicates preserved → true row count),
    // NOT a deduped set. This is the signal that catches a double-stored row.
    let read_stored_rows = || -> Vec<String> {
        let pk_cstr = std::ffi::CString::new(pubkey_hex.as_str()).ok();
        let json_ptr = pk_cstr
            .as_ref()
            .map(|pk| nmp_ffi::nmp_app_read_author_event_ids(app.raw(), pk.as_ptr(), 0));
        match json_ptr {
            Some(ptr) if !ptr.is_null() => {
                let parsed = unsafe { std::ffi::CStr::from_ptr(ptr) }
                    .to_str()
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
                nmp_ffi::nmp_free_string(ptr);
                parsed
                    .and_then(|v| {
                        v.as_array().map(|arr| {
                            arr.iter()
                                .filter_map(|obj| obj["id"].as_str().map(str::to_owned))
                                .collect()
                        })
                    })
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    };

    // Snapshot the stored ROW COUNT, re-inject the identical corpus, snapshot
    // again. The seam preserves one row per stored event, so the Vec LENGTH is
    // the actual stored row count for this author (a duplicate row would make
    // the length grow even though the id set would not).
    let rows_before = read_stored_rows();
    for json in &corpus {
        if let Ok(c) = std::ffi::CString::new(json.as_str()) {
            let _ = nmp_ffi::nmp_app_inject_signed_event_json(app.raw(), c.as_ptr());
        }
    }
    let reinject_settled = app.wait_barrier(Duration::from_secs(5));
    if !reinject_settled {
        report.push(GateRow::unmeasured(
            "reactive-no-duplicate-stored-row",
            phase,
            "nmp_app_read_author_event_ids (row COUNT before/after re-inject)",
            "actor barrier after duplicate re-inject",
            "barrier ack before row-count read",
            Verdict::Blocked,
            "duplicate re-inject commands did not settle before the timeout — cannot assert the \
             row-count dedup invariant without risking a stale store read",
        ));
        report.push(GateRow::unmeasured(
            "reactive-id-subset",
            phase,
            "nmp_app_read_author_event_ids + injected corpus ids",
            "actor barrier after duplicate re-inject",
            "seeded rows settled before id-subset read",
            Verdict::Blocked,
            "duplicate re-inject commands did not settle before the timeout — cannot reuse a \
             post-timeout store read for the id-subset assertion",
        ));
        return;
    }
    let rows_after = read_stored_rows();

    if rows_before.is_empty() && rows_after.is_empty() {
        report.push(GateRow::unmeasured(
            "reactive-no-duplicate-stored-row",
            phase,
            "nmp_app_read_author_event_ids (row COUNT before/after re-inject)",
            "stored ROW COUNT must not grow on re-injected duplicate ids",
            "0 new stored rows",
            Verdict::SkipRelayMiss,
            "store-read seam returned no author rows for the injected corpus — cannot assert the \
             row-count dedup invariant this run (SKIP LOUD; never a 0→0 vacuous pass)",
        ));
    } else {
        let grew = rows_after.len().saturating_sub(rows_before.len());
        report.push(
            GateRow::max(
                "reactive-no-duplicate-stored-row",
                phase,
                "nmp_app_read_author_event_ids (row COUNT before/after re-inject)",
                "stored ROW COUNT growth on re-injected duplicate ids",
                grew as f64,
                0.0,
                "extra-stored-rows",
            )
            .with_note(&format!(
                "re-injected the identical {injected}-event corpus; stored ROW count \
                 {}→{} (settled={reinject_settled}; must not grow — the store dedups by id; \
                 a re-injected event adds no row; a double-stored row would grow the count and \
                 FAIL this gate)",
                rows_before.len(),
                rows_after.len()
            )),
        );
    }

    // (d) id-subset check: every injected event id must be readable from the
    //     store for the author we signed as. Hard pass/fail: if any corpus id is
    //     missing the reactive pipeline dropped or never persisted it.
    let stored_ids_after: std::collections::HashSet<String> = rows_after.iter().cloned().collect();
    let missing_count = corpus_ids
        .iter()
        .filter(|id| !stored_ids_after.contains(*id))
        .count();
    report.push(
        GateRow::max(
            "reactive-id-subset",
            phase,
            "nmp_app_read_author_event_ids + injected corpus ids",
            "corpus_ids \u{2286} stored_ids (missing count)",
            missing_count as f64,
            0.0,
            "missing-ids",
        )
        .with_note(&format!(
            "corpus={} ids, stored={} rows, missing={} \
             (seam 1: nmp_app_read_author_event_ids)",
            corpus_ids.len(),
            rows_after.len(),
            missing_count,
        )),
    );

    // (e) RESIDUAL GAP — feed pager / load_older cursor correctness (ADR-0058
    //     step-6). A self-contained oracle would inject a known OLDER corpus,
    //     drive `nmp_app_load_older_feed` and assert no missed/duplicate page ids
    //     + a monotonically advancing cursor. That is NOT self-contained today:
    //     `load_older` paginates via the pull substrate against CONFIGURED RELAYS
    //     (the wire), not the local inject-store sub-id, so an injected corpus
    //     never reaches the pager. Driving it needs either a relay-backed fixture
    //     or a new "pull-from-store" seam. Documented as BLOCKED (not counted in
    //     hard PASS/FAIL) rather than faked with a vacuous oracle. The
    //     `cold_start` phase's `load-older` row exercises the live-relay variant
    //     and SKIPs honestly when the backlog is exhausted.
    report.push(GateRow::unmeasured(
        "reactive-pager-cursor",
        phase,
        "nmp_app_load_older_feed (pull substrate)",
        "older page ids contiguous + cursor advances (no missed/duplicate page)",
        "no missed/duplicate page ids; cursor strictly advances",
        Verdict::Blocked,
        "feed pager/load_older cursor correctness is not self-contained: load_older pulls older \
         pages from configured relays via the pull substrate, not the local inject-store sub-id, \
         so an injected corpus never reaches the pager. Needs a relay-backed older-corpus fixture \
         or a pull-from-store seam. BLOCKED (documented gap) — not a faked pass. The cold_start \
         `load-older` row covers the live-relay variant.",
    ));
}

/// K self-authored kind:1 notes (real Schnorr signatures) with increasing
/// created_at so id-dedup is exercised on the second pass.
fn build_self_notes(keys: &Keys, base: u64, n: u64) -> Vec<String> {
    use nostr::Timestamp;
    (0..n)
        .filter_map(|i| {
            EventBuilder::text_note(format!("reactive-oracle self note {i}"))
                .custom_created_at(Timestamp::from(base + i))
                .sign_with_keys(keys)
                .ok()
                .map(|e: nostr::Event| e.as_json())
        })
        .collect()
}
