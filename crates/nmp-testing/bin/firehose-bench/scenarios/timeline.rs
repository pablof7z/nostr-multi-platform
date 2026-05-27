//! Timeline scenarios: cold_start, sustained_firehose, soak.

use crate::config::{
    Scale, DELTAS_PER_VIEW_SEC_GATE, FILLED_TIMELINE_GATE_MS, FIRST_ITEM_GATE_MS,
    INGEST_TO_EMIT_P99_GATE_MS, MEMORY_DRIFT_30M_GATE_MB, RAMP_MEMORY_GATE_MB, SOAK_MEMORY_GATE_MB,
    VIEW_BATCH_HZ_GATE,
};
use crate::report::{ScenarioMetrics, ScenarioResult};
use crate::scenarios::{finish_scenario, gate_eq, gate_eq_i64, gate_max, round2, Lcg};
use std::time::Instant;

pub(crate) fn cold_start(scale: Scale) -> ScenarioResult {
    let started = Instant::now();
    let events = (10_000.0 * scale.factor()) as u64;
    let mut checksum = 0_u64;
    for i in 0..events {
        checksum ^= i
            .wrapping_mul(6364136223846793005)
            .rotate_left((i % 63) as u32);
    }
    let first_item_ms = 42.0 + (events as f64 / 10_000.0) * 18.0 + (checksum % 7) as f64;
    let filled_timeline_ms = 480.0 + (events as f64 / 10_000.0) * 260.0;
    let peak_memory_mb = 48.0 + (events as f64 / 10_000.0) * 8.0;
    let metrics = ScenarioMetrics {
        first_item_ms: Some(round2(first_item_ms)),
        filled_timeline_ms: Some(round2(filled_timeline_ms)),
        peak_memory_mb: Some(round2(peak_memory_mb)),
        trace_records: Some(events),
        synthetic_runtime_ms: Some(started.elapsed().as_millis()),
        ..ScenarioMetrics::default()
    };
    finish_scenario(
        "cold_start",
        "Empty store startup into first timeline paint and filled timeline.",
        5,
        events,
        metrics,
        vec![
            gate_max("first_item_ms", first_item_ms, FIRST_ITEM_GATE_MS, None),
            gate_max(
                "filled_timeline_ms",
                filled_timeline_ms,
                FILLED_TIMELINE_GATE_MS,
                None,
            ),
            gate_max("peak_memory_mb", peak_memory_mb, RAMP_MEMORY_GATE_MB, None),
        ],
        vec!["Modeled cold start is comfortably under the proposed gates; real storage open and FFI startup still need measurement.".to_string()],
    )
}

pub(crate) fn sustained_firehose(scale: Scale) -> ScenarioResult {
    let started = Instant::now();
    let virtual_duration_seconds = match scale {
        Scale::Quick => 300,
        Scale::Standard => 1_800,
    };
    let event_rate = 500_u64;
    let events = virtual_duration_seconds * event_rate;
    let mut rng = Lcg::new(0x5155_5354_4149_4e44);
    let mut max_burst = 0_u64;
    let mut checksum = 0_u64;
    for _ in 0..(events.min(250_000)) {
        let burst = 1 + rng.next_mod(9);
        max_burst = max_burst.max(burst);
        checksum ^= rng.next();
    }
    let ingest_to_emit_p99_ms = 13.0 + max_burst as f64 * 1.4;
    let view_batch_hz = 58.0;
    let max_deltas_per_view_sec = 57.0;
    let memory_drift_mb = 22.0 + scale.factor() * 11.0;
    let relay_connections = 54;
    let metrics = ScenarioMetrics {
        memory_drift_mb: Some(round2(memory_drift_mb)),
        ingest_to_emit_p99_ms: Some(round2(ingest_to_emit_p99_ms)),
        view_batch_hz: Some(view_batch_hz),
        max_deltas_per_view_sec: Some(max_deltas_per_view_sec),
        dropped_events: Some(0),
        relay_connections: Some(relay_connections),
        trace_records: Some(events),
        synthetic_runtime_ms: Some(started.elapsed().as_millis()),
        ..ScenarioMetrics::default()
    };
    finish_scenario(
        "sustained_firehose",
        "Following timeline plus hashtag firehose under sustained bursty traffic.",
        virtual_duration_seconds,
        events,
        metrics,
        vec![
            gate_max(
                "memory_drift_mb",
                memory_drift_mb,
                MEMORY_DRIFT_30M_GATE_MB,
                None,
            ),
            gate_eq("dropped_events", 0, 0, None),
            gate_max(
                "ingest_to_emit_p99_ms",
                ingest_to_emit_p99_ms,
                INGEST_TO_EMIT_P99_GATE_MS,
                None,
            ),
            gate_max("view_batch_hz", view_batch_hz, VIEW_BATCH_HZ_GATE, None),
            gate_max(
                "max_deltas_per_view_sec",
                max_deltas_per_view_sec,
                DELTAS_PER_VIEW_SEC_GATE,
                Some("ADR-0002 per-view budget".to_string()),
            ),
            gate_max("relay_connections", relay_connections as f64, 80.0, None),
        ],
        vec![
            "The modeled pipeline only passes because ADR-0002 per-view coalescing is assumed."
                .to_string(),
            format!("Synthetic burst checksum retained to prevent optimization: {checksum:x}"),
        ],
    )
}

pub(crate) fn soak(scale: Scale) -> ScenarioResult {
    let virtual_duration_seconds = match scale {
        Scale::Quick => 3_600,
        Scale::Standard => 86_400,
    };
    let event_rate = 25_u64;
    let events = virtual_duration_seconds * event_rate;
    let memory_growth = match scale {
        Scale::Quick => 5.2,
        Scale::Standard => 38.0,
    };
    let fd_growth = 0;
    let panics = 0;
    let view_batch_hz = 44.0;
    let metrics = ScenarioMetrics {
        memory_drift_mb: Some(memory_growth),
        fd_growth: Some(fd_growth),
        panics: Some(panics),
        view_batch_hz: Some(view_batch_hz),
        trace_records: Some(events),
        ..ScenarioMetrics::default()
    };
    finish_scenario(
        "soak",
        "Virtual 24-hour alternating workload for long-run memory and descriptor drift.",
        virtual_duration_seconds,
        events,
        metrics,
        vec![
            gate_max("memory_growth_mb", memory_growth, SOAK_MEMORY_GATE_MB, None),
            gate_eq_i64("fd_growth", fd_growth, 0, None),
            gate_eq("panics", panics, 0, None),
            gate_max("view_batch_hz", view_batch_hz, VIEW_BATCH_HZ_GATE, None),
        ],
        vec!["Real soak still must run in live mode once relay/storage/runtime exist.".to_string()],
    )
}
