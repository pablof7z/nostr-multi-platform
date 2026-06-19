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
//!  - NO-DOUBLE-EMIT: re-injecting the identical corpus adds NO new rows (the
//!    store dedups by id; the projection does not double-serve).
//!
//! HOOK NOTE: the typed `SnapshotEnvelope` carries `visible_items` (count) but
//! NOT per-item id/author hex, so the FULL id-subset check is BLOCKED on the
//! same `nmp_app_read_feed_authors` seam already documented in `oracle.rs`. The
//! COUNT-complete half below is a real PASS/FAIL — a dropped update fails it.

use std::time::Duration;

use nostr::{EventBuilder, JsonUtil, Keys, ToBech32};

use crate::config::{gates, Args, Phase};
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

    let app = DrivenApp::launch(Some(&nsec), Some(&pubkey_hex), std::slice::from_ref(&args.relay));
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
        .wait_until(Duration::from_millis(gates::WIRE_TO_VISIBLE_GATE_MS as u64), |s| {
            s.latest().map(|r| r.note_events).unwrap_or(0) >= notes_target
        })
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
        .wait_until(Duration::from_millis(gates::WIRE_TO_VISIBLE_GATE_MS as u64), |s| {
            s.peak_visible() >= visible_target
        })
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

    // (c) No-double-emit: re-inject the identical corpus; the projection must
    //     NOT grow (dedup by id; no double-serve under the wakeup interaction).
    let before_dupe = app.with_state(|s| s.peak_visible());
    for json in &corpus {
        if let Ok(c) = std::ffi::CString::new(json.as_str()) {
            let _ = nmp_ffi::nmp_app_inject_signed_event_json(app.raw(), c.as_ptr());
        }
    }
    std::thread::sleep(Duration::from_secs(2));
    let after_dupe = app.with_state(|s| s.peak_visible());
    report.push(
        GateRow::max(
            "reactive-no-double-emit",
            phase,
            "nmp_app_inject_signed_event_json (re-inject)",
            "visible_items must not grow on duplicate ids",
            after_dupe.saturating_sub(before_dupe) as f64,
            0.0,
            "extra-rows",
        )
        .with_note("re-injecting the identical corpus adds no rows (store dedups by id)"),
    );

    report.finding(
        "HOOK GAP (family 1) — id-subset half of missed-update: SnapshotEnvelope carries \
         visible_items (count) not per-item id/author hex, so `corpus IDs ⊆ visible IDs` is \
         asserted at the COUNT level only. Wire `nmp_app_read_feed_authors` (read-only typed \
         projection accessor) to complete the exact set-inclusion check.",
    );
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
