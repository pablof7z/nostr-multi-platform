//! Robustness family 4 — FFI BOUNDEDNESS + PANIC SAFETY.
//!
//! aim.md §4.1 "the full store never crosses FFI" + invariant 2 "errors do not
//! cross FFI". These oracles drive the kernel through the public seam and read
//! the raw frame size off the capture path (`payload_len`) + actor liveness.
//!
//! Falsifiable hypotheses:
//!  - FRAME BOUNDED: as the store grows well past the projection cap, the raw
//!    FlatBuffers snapshot frame MUST stay below an absolute byte ceiling. A
//!    frame that scales with store size is the unbounded-projection bug.
//!  - PANIC SAFETY: a malformed event MUST NOT cross FFI (inject returns false)
//!    and the actor thread MUST survive — no panic unwinds the boundary.
//!
//! NOTE: a true ≥100k replay is an orchestrator job (local `nak serve` corpus,
//! see scripts/perf-sanity). The in-process burst below is the bounded-frame
//! proxy: the projection cap (visible_limit 500) is exactly the mechanism that
//! keeps the frame bounded as the store grows, and the absolute ceiling holds
//! regardless of store size.

use std::time::Duration;

use crate::config::{gates, Args, Phase};
use crate::report::{GateRow, SanityReport, Verdict};

/// First store size (already well past the 500 projection cap).
const STORE_SIZE_SMALL: u32 = 1_000;
/// Additional events to take the store ~20x larger for the SLOPE check.
const STORE_SIZE_LARGE_ADD: u32 = 19_000;
/// Allowed growth of the steady-state frame between the small and large store
/// sizes. The projection is capped at 500 visible items, so the frame must stay
/// essentially FLAT as the store grows ~20x; a full-store-crosses-FFI regression
/// would balloon the frame by megabytes. 64 KiB covers cap jitter + a few extra
/// Tier-3 rows without admitting a store-scaling frame.
const FRAME_SLOPE_TOLERANCE_BYTES: f64 = 65_536.0;

/// Latest (steady-state) frame size — reflects the CURRENT projection, not the
/// monotonic peak, so a store that grows without growing the frame reads flat.
fn latest_frame_bytes(app: &crate::driver::DrivenApp) -> u64 {
    app.with_state(|s| s.latest().map(|r| r.frame_bytes).unwrap_or(0))
}

fn frame_count(app: &crate::driver::DrivenApp) -> usize {
    app.with_state(|s| s.records.len())
}

fn wait_for_next_frame(app: &crate::driver::DrivenApp, before: usize) {
    let _ = app.wait_until(Duration::from_secs(5), |s| s.records.len() > before);
}

pub fn run_ffi_bounds(report: &mut SanityReport, args: &Args) {
    let phase = Phase::FfiBounds.as_str();
    let Some(app) = super::connect_or_skip_optional(report, phase, args) else {
        return;
    };

    // ── SLOPE check: grow the store ~20x; the steady-state frame must NOT grow
    //    with it. The prior gate injected 5k small events and only checked a 4 MB
    //    absolute ceiling — a full-store-crosses-FFI regression could stay under
    //    that. Measuring the frame at two store sizes makes the gate FAIL if the
    //    frame tracks the store instead of the 500-item projection cap.
    let before_small = frame_count(&app);
    nmp_ffi::nmp_app_inject_signed_events(app.raw(), crate::report::now_unix(), STORE_SIZE_SMALL);
    wait_for_next_frame(&app, before_small);
    let frame_small = latest_frame_bytes(&app);

    let before_large = frame_count(&app);
    nmp_ffi::nmp_app_inject_signed_events(
        app.raw(),
        crate::report::now_unix() + 1,
        STORE_SIZE_LARGE_ADD,
    );
    wait_for_next_frame(&app, before_large);
    let frame_large = latest_frame_bytes(&app);
    let frame_peak = app.with_state(|s| s.peak_frame_bytes());

    if frame_small == 0 || frame_large == 0 {
        report.push(GateRow::unmeasured(
            "ffi-frame-bounded",
            phase,
            "nmp_app_inject_signed_events + capture payload_len",
            "steady-state SnapshotFrame bytes at two store sizes",
            "frame must not grow with the store",
            Verdict::Blocked,
            &format!(
                "no steady-state frame captured (small={frame_small}, large={frame_large}) — \
                 capture path saw no snapshot frame after a store-growth burst (BLOCKED)"
            ),
        ));
    } else {
        let slope = frame_large.saturating_sub(frame_small);
        report.push(
            GateRow::max(
                "ffi-frame-bounded",
                phase,
                "nmp_app_inject_signed_events (1k vs 20k) + capture payload_len",
                "steady-state frame-bytes GROWTH as the store grows ~20x",
                slope as f64,
                FRAME_SLOPE_TOLERANCE_BYTES,
                "bytes-delta",
            )
            .with_note(&format!(
                "store {STORE_SIZE_SMALL}→{} events (~20x); steady-state frame {frame_small}→{frame_large} \
                 bytes (delta={slope}) — must stay flat (frame tracks the 500-item projection cap, \
                 NOT store size; the full store never crosses FFI)",
                STORE_SIZE_SMALL + STORE_SIZE_LARGE_ADD
            )),
        );
    }

    // Absolute ceiling still holds regardless of store size (kept as a second
    // real gate, not the primary boundedness proof).
    report.push(
        GateRow::max(
            "ffi-frame-under-ceiling",
            phase,
            "capture payload_len (peak)",
            "peak SnapshotFrame bytes after store growth",
            frame_peak as f64,
            gates::FRAME_BYTES_GATE,
            "bytes",
        )
        .with_note(&format!(
            "peak frame bytes after ~20k-event store = {frame_peak}; absolute ceiling holds \
             regardless of store size"
        )),
    );

    // PANIC SAFETY: malformed inputs must not cross FFI nor kill the actor.
    let alive_before = app.is_alive();
    let malformed = [
        "{ not valid json",
        "{}",
        "{\"id\":\"zz\",\"sig\":\"00\"}",
        "[]",
        "null",
    ];
    let mut any_accepted = false;
    for m in malformed {
        if let Ok(c) = std::ffi::CString::new(m) {
            if nmp_ffi::nmp_app_inject_signed_event_json(app.raw(), c.as_ptr()) {
                any_accepted = true;
            }
        }
    }
    let _ = app.wait_until(Duration::from_secs(1), |_| !app.is_alive());
    let alive_after = app.is_alive();

    report.push(
        GateRow::max(
            "ffi-malformed-rejected",
            phase,
            "nmp_app_inject_signed_event_json (malformed inputs)",
            "inject return value for malformed JSON",
            if any_accepted { 1.0 } else { 0.0 },
            0.0,
            "accepted-malformed",
        )
        .with_note("malformed/garbage event JSON must be rejected at the boundary (no accept)"),
    );
    report.push(
        GateRow::min(
            "ffi-actor-survives-panic-input",
            phase,
            "nmp_app_is_alive",
            "actor thread liveness across malformed inject",
            if alive_after { 1.0 } else { 0.0 },
            1.0,
            "alive",
        )
        .with_note(&format!(
            "actor alive before={alive_before} after={alive_after} — errors must not cross FFI"
        )),
    );
}
