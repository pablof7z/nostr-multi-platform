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

/// In-process burst size to grow the store well past the 500 projection cap.
const STORE_GROWTH_BURST: u32 = 5_000;

pub fn run_ffi_bounds(report: &mut SanityReport, args: &Args) {
    let phase = Phase::FfiBounds.as_str();
    let Some(app) = super::connect_or_skip_optional(report, phase, args) else {
        return;
    };

    let frame_before = app.with_state(|s| s.peak_frame_bytes());

    // Grow the store far past the projection cap with real signed events.
    nmp_ffi::nmp_app_inject_signed_events(app.raw(), crate::report::now_unix(), STORE_GROWTH_BURST);
    // Let the projection ticks flush the growth.
    std::thread::sleep(Duration::from_secs(5));
    let frame_after = app.with_state(|s| s.peak_frame_bytes());

    report.push(
        GateRow::max(
            "ffi-frame-bounded",
            phase,
            "nmp_app_inject_signed_events + capture payload_len",
            "max SnapshotFrame bytes after store growth",
            frame_after as f64,
            gates::FRAME_BYTES_GATE,
            "bytes",
        )
        .with_note(&format!(
            "injected {STORE_GROWTH_BURST} events (store grows past the 500 projection cap); \
             peak frame bytes {frame_before}→{frame_after} — must stay under the ceiling \
             regardless of store size (the full store never crosses FFI)"
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
    std::thread::sleep(Duration::from_secs(1));
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

    if frame_after == 0 {
        report.push(GateRow::unmeasured(
            "ffi-frame-bounded-note",
            phase,
            "capture payload_len",
            "SnapshotFrame bytes",
            "non-zero frame captured",
            Verdict::Blocked,
            "no frame bytes captured — capture path saw no snapshot frame (BLOCKED)",
        ));
    }
}
