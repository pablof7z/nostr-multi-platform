//! Firehose phase + no-dropped-events oracle against a KNOWN local corpus.
//!
//! Strategy: rather than depend on a relay delivering a precise count (which we
//! cannot assert against), we inject a KNOWN corpus directly through the
//! kernel's test-support ingest seam (`nmp_app_inject_signed_event_json`) — the
//! same path a relay EVENT frame takes after Schnorr verify — and assert every
//! distinct event becomes visible (no drops) within budget, while measuring the
//! ingest→emit latency from the captured frame timeline.
//!
//! The corpus is read from `artifacts/real-events.jsonl` (captured via `nak req`
//! by the orchestrator) when present, else a self-signed synthetic burst so the
//! phase is runnable on bare master without external capture.
//!
//! D0: `nmp_app_inject_signed_event_json` is the documented #[cfg(test-support)]
//! injection escape hatch — it routes through full Schnorr verify, exactly as a
//! relay delivery would. It is NOT part of the production FFI ABI.

use std::ffi::CString;
use std::time::{Duration, Instant};

use nmp_ffi::nmp_app_inject_signed_event_json;
use nostr::{EventBuilder, JsonUtil, Keys, Timestamp};

use crate::config::{gates, Args, Phase};
use crate::report::{GateRow, SanityReport, Verdict};

/// How many events the synthetic firehose burst injects when no captured
/// corpus is present.
const SYNTHETIC_BURST: usize = 300;
/// Budget for all injected events to become visible.
const VISIBLE_BUDGET: Duration = Duration::from_secs(20);

pub fn run_firehose(report: &mut SanityReport, args: &Args) {
    let phase = Phase::Firehose.as_str();
    // The firehose oracle drives the kernel ingest seam directly, so it does
    // not strictly require a relay — but we still launch the real app so the
    // full composition (feed engine, projections, GC) is exercised.
    let Some(app) = super::connect_or_skip_optional(report, phase, args) else {
        return;
    };

    let corpus = load_corpus();
    let injected = corpus.len();
    if injected == 0 {
        report.push(GateRow::unmeasured(
            "no-dropped-events",
            phase,
            "nmp_app_inject_signed_event_json",
            "artifacts/real-events.jsonl",
            "visible == injected",
            Verdict::Blocked,
            "no corpus: capture via scripts/perf-sanity capture step, or rely on synthetic burst",
        ));
        return;
    }

    // Count the kind:1 notes in the corpus — the kernel's `note_events` counter
    // is the wire-agnostic no-drop signal (the relay-only `events_rx` stays 0
    // for the injection seam, which is correct: injection is not a wire arrival).
    let kind1 = corpus.iter().filter(|j| is_kind1(j)).count() as u64;
    let notes_before = app.with_state(|s| s.latest().map(|r| r.note_events).unwrap_or(0));
    let rx_before = app.with_state(|s| s.latest().map(|r| r.events_rx).unwrap_or(0));
    let t0 = Instant::now();
    let mut accepted = 0usize;
    for json in &corpus {
        if let Ok(c) = CString::new(json.as_str()) {
            if nmp_app_inject_signed_event_json(app.raw(), c.as_ptr()) {
                accepted += 1;
            }
        }
    }

    // No-dropped-events oracle: every kind:1 note in the corpus must surface in
    // the kernel's `note_events` counter. This is NOT follow-gated (unlike
    // `visible_items`) and proves the firehose did not silently drop notes
    // between the ingest seam and the kernel store.
    let target_notes = notes_before + kind1;
    let reached = app
        .wait_until(VISIBLE_BUDGET, |s| {
            s.latest().map(|r| r.note_events).unwrap_or(0) >= target_notes
        })
        .is_some();
    let (final_notes, final_rx, first_frame_ms) = app.with_state(|s| {
        let last = s.latest();
        (
            last.map(|r| r.note_events).unwrap_or(0),
            last.map(|r| r.events_rx).unwrap_or(0),
            s.records.first().map(|r| r.at_ms).unwrap_or(0),
        )
    });
    let rendered = corpus.iter().filter(|j| is_timeline_kind(j)).count() as u64;
    let elapsed_ms = t0.elapsed().as_millis() as u64;

    // No-dropped-events gate: note_events delta must reach the kind:1 count.
    let drops = target_notes.saturating_sub(final_notes);
    report.push(
        GateRow::max(
            "no-dropped-events",
            phase,
            "nmp_app_inject_signed_event_json + decode_snapshot_envelope",
            "SnapshotEnvelope.note_events delta vs injected kind:1 count",
            drops as f64,
            0.0,
            "dropped",
        )
        .with_note(&format!(
            "injected {injected} ({accepted} Schnorr-valid, {kind1} kind:1, {rendered} timeline-kind); \
             note_events {notes_before}→{final_notes}, events_rx {rx_before}→{final_rx} \
             (events_rx=0 is correct: injection is not a wire arrival), \
             first_frame_at={first_frame_ms}ms (reached={reached})"
        )),
    );
    let final_visible = app.with_state(|s| s.peak_visible());

    // ingest→emit latency proxy: time from first inject to all-visible, divided
    // across the rendered batch. This is a coarse upper bound, not a per-event
    // p99 (the per-event ingest→emit timestamp hook is #[cfg(test)] only — see
    // findings). Gate against the firehose INGEST_TO_EMIT_P99 budget as a
    // ceiling on the AMORTISED per-event emit cost.
    let per_event_ms = if kind1 > 0 {
        elapsed_ms as f64 / kind1 as f64
    } else {
        0.0
    };
    report.push(
        GateRow::max(
            "ingest-to-emit-amortised",
            phase,
            "wall clock (inject→visible)",
            "decode_snapshot_envelope visible_items",
            per_event_ms,
            gates::INGEST_TO_EMIT_P99_GATE_MS,
            "ms/event",
        )
        .with_note(
            "AMORTISED, not true p99 — per-event ingest→emit timestamp hook is #[cfg(test)] only \
             (HOOK MISSING; wire a process-lifetime ingest-latency histogram in a follow-up).",
        ),
    );

    // Dedup/provenance oracle: re-inject the SAME corpus. The visible feed must
    // NOT grow (store dedups by event id, merging provenance on the duplicate).
    // We assert on visible_items (rendered rows) rather than events_rx, because
    // events_rx is a raw arrival counter that legitimately ticks on every
    // delivery; the dedup invariant is "no NEW rows from duplicate ids".
    for json in &corpus {
        if let Ok(c) = CString::new(json.as_str()) {
            let _ = nmp_app_inject_signed_event_json(app.raw(), c.as_ptr());
        }
    }
    let _ = app.wait_until(Duration::from_secs(2), |s| s.peak_visible() > final_visible);
    let visible_after_dupe = app.with_state(|s| s.peak_visible());
    report.push(
        GateRow::max(
            "dedup-no-growth-on-duplicate",
            phase,
            "nmp_app_inject_signed_event_json (re-inject)",
            "visible_items must not grow on duplicate ids",
            visible_after_dupe.saturating_sub(final_visible) as f64,
            0.0,
            "extra-rows",
        )
        .with_note("store dedups by event id; a second arrival of the same corpus adds no rows"),
    );

    push_encode_and_alloc_rows(report, phase, &app);
    push_load_cpu_soft_flag(report, phase, args);
}

/// Surface the in-process encode-time p99 (from `serialize_us`) and the
/// allocation footprint over the firehose. These are CONTEXT rows (not hard
/// gates), proving the encode path did not blow up under the burst.
fn push_encode_and_alloc_rows(
    report: &mut SanityReport,
    phase: &str,
    app: &crate::driver::DrivenApp,
) {
    use crate::metrics::{alloc_snapshot, percentile};
    let mut encode_us: Vec<f64> = app.with_state(|s| {
        s.records
            .iter()
            .filter(|r| r.serialize_us > 0)
            .map(|r| r.serialize_us as f64)
            .collect()
    });
    let p99_ms = percentile(&mut encode_us, 99.0) / 1000.0;
    report.push(
        GateRow::max(
            "encode-p99",
            phase,
            "decode_snapshot_envelope",
            "SnapshotEnvelope.serialize_us (p99)",
            p99_ms,
            gates::INGEST_TO_EMIT_P99_GATE_MS,
            "ms",
        )
        .with_note("per-tick FlatBuffers encode time (one-tick-lag field); must stay well under the emit budget"),
    );

    let alloc = alloc_snapshot();
    report.push(
        GateRow::min(
            "alloc-progress",
            phase,
            "CountingAllocator (#[global_allocator])",
            "metrics::alloc_snapshot",
            alloc.allocations as f64,
            1.0,
            "allocs",
        )
        .with_note(&format!(
            "process allocations={} peak_heap={:.1}MB — context for the memory phase",
            alloc.allocations,
            alloc.peak_heap_bytes as f64 / (1024.0 * 1024.0)
        )),
    );
}

/// Soft-flag (informational): under sustained firehose, process CPU above the
/// soft ceiling is "hot" — legitimate for a real firehose, but a value near a
/// full core WHILE IDLE is the bug we hunt (covered by the idle-cpu gate).
fn push_load_cpu_soft_flag(report: &mut SanityReport, phase: &str, args: &Args) {
    let Some(peak) = args
        .os_metrics_path
        .as_deref()
        .and_then(|p| crate::metrics::load_os_metrics(p, phase))
        .and_then(|o| o.cpu_pct_peak)
    else {
        return;
    };
    let verdict = if peak <= gates::LOAD_CPU_PCT_SOFT_FLAG {
        Verdict::Pass
    } else {
        // Soft: do not fail the run; surface as a noted PASS-with-flag via Blocked-style note.
        Verdict::Pass
    };
    let mut row = GateRow::max(
        "load-cpu-soft",
        phase,
        "ps -o %cpu / top sampling (sidecar)",
        "scripts/perf-sanity cpu_pct_peak",
        peak,
        gates::LOAD_CPU_PCT_SOFT_FLAG,
        "%",
    );
    row.verdict = verdict;
    report.push(row.with_note(
        "SOFT flag only — a real firehose legitimately uses CPU; this row never fails the run. \
         The hard CPU detector is the idle-cpu gate.",
    ));
}

/// Load the captured corpus, or synthesise a signed burst so the phase runs on
/// bare master. Each line is a NIP-01 event JSON.
fn load_corpus() -> Vec<String> {
    if let Ok(raw) = std::fs::read_to_string("artifacts/real-events.jsonl") {
        let lines: Vec<String> = raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && l.starts_with('{'))
            .map(str::to_string)
            .collect();
        if !lines.is_empty() {
            return lines;
        }
    }
    synthetic_burst(SYNTHETIC_BURST)
}

/// Self-signed kind:1 burst (real Schnorr signatures so the verify path runs).
fn synthetic_burst(n: usize) -> Vec<String> {
    let keys = Keys::generate();
    let base = crate::report::now_unix();
    (0..n)
        .filter_map(|i| {
            EventBuilder::text_note(format!("sanity-firehose corpus event {i}"))
                .custom_created_at(Timestamp::from(base + i as u64))
                .sign_with_keys(&keys)
                .ok()
                .map(|e: nostr::Event| e.as_json())
        })
        .collect()
}

/// True if the event JSON is a rendered timeline kind (1 = note, 6 = repost).
fn is_timeline_kind(json: &str) -> bool {
    nostr::Event::from_json(json)
        .map(|e| matches!(e.kind.as_u16(), 1 | 6))
        .unwrap_or(false)
}

/// True if the event JSON is a kind:1 note (the `note_events` counter's domain).
fn is_kind1(json: &str) -> bool {
    nostr::Event::from_json(json)
        .map(|e| e.kind.as_u16() == 1)
        .unwrap_or(false)
}
