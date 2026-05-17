use crate::config::{
    Scale, DELTAS_PER_VIEW_SEC_GATE, DISCONNECT_DETECT_GATE_MS, FILLED_TIMELINE_GATE_MS,
    FIRST_ITEM_GATE_MS, INGEST_TO_EMIT_P99_GATE_MS, MEMORY_DRIFT_30M_GATE_MB,
    NIP77_BYTES_RATIO_GATE, NSE_DECRYPT_GATE_MS, NSE_MEMORY_GATE_MB, RAMP_MEMORY_GATE_MB,
    RECONNECT_GATE_MS, SOAK_MEMORY_GATE_MB, VIEW_BATCH_HZ_GATE,
};
use crate::report::{GateResult, ScenarioMetrics, ScenarioResult};
use std::time::Instant;

pub(crate) fn run_scenario(name: &'static str, scale: Scale) -> ScenarioResult {
    match name {
        "cold_start" => cold_start(scale),
        "sustained_firehose" => sustained_firehose(scale),
        "profile_thrashing" => profile_thrashing(scale),
        "relay_disconnect_storm" => relay_disconnect_storm(scale),
        "multi_account" => multi_account(scale),
        "negentropy_efficiency" => negentropy_efficiency(scale),
        "background_decryption" => background_decryption(scale),
        "soak" => soak(scale),
        _ => unreachable!("selected_scenarios validates names"),
    }
}

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

pub(crate) fn profile_thrashing(scale: Scale) -> ScenarioResult {
    let virtual_duration_seconds = match scale {
        Scale::Quick => 120,
        Scale::Standard => 600,
    };
    let mount_rate = 50.0;
    let transitions = (virtual_duration_seconds as f64 * mount_rate) as u64;
    let dispatch_rate = 22.5;
    let hit_rate = 0.68;
    let memory_drift_mb = 0.4;
    let leaks = 0;
    let metrics = ScenarioMetrics {
        open_close_dispatch_rate_per_sec: Some(dispatch_rate),
        mount_unmount_rate_per_sec: Some(mount_rate),
        projection_cache_hit_rate: Some(hit_rate),
        memory_drift_mb: Some(memory_drift_mb),
        leaked_subscriptions: Some(leaks),
        trace_records: Some(transitions),
        ..ScenarioMetrics::default()
    };
    finish_scenario(
        "profile_thrashing",
        "Simulated fast scroll mounting and unmounting profile/avatar wrappers.",
        virtual_duration_seconds,
        transitions,
        metrics,
        vec![
            gate_max("memory_drift_mb", memory_drift_mb, 1.0, None),
            gate_max(
                "open_close_dispatch_rate_per_sec",
                dispatch_rate,
                mount_rate * 0.60,
                Some("ADR-0005 wrapper dedupe/grace-period absorption".to_string()),
            ),
            gate_min("projection_cache_hit_rate", hit_rate, 0.50, None),
            gate_eq("leaked_subscriptions", leaks, 0, None),
        ],
        vec!["ADR-0005 domain-keyed wrappers are the right primitive for this scenario; ViewId-keyed shadows would make dedupe much harder.".to_string()],
    )
}

pub(crate) fn relay_disconnect_storm(scale: Scale) -> ScenarioResult {
    let virtual_duration_seconds = match scale {
        Scale::Quick => 180,
        Scale::Standard => 1_800,
    };
    let disconnects = match scale {
        Scale::Quick => 8,
        Scale::Standard => 64,
    };
    let detect_p99 = 4_200.0;
    let reconnect_p99 = 18_500.0;
    let event_loss = 0;
    let metrics = ScenarioMetrics {
        detect_disconnect_p99_ms: Some(detect_p99),
        reconnect_p99_ms: Some(reconnect_p99),
        event_loss: Some(event_loss),
        trace_records: Some(disconnects),
        ..ScenarioMetrics::default()
    };
    finish_scenario(
        "relay_disconnect_storm",
        "Synthetic relay flapping with resubscribe and gap-fill behavior.",
        virtual_duration_seconds,
        disconnects,
        metrics,
        vec![
            gate_max(
                "detect_disconnect_p99_ms",
                detect_p99,
                DISCONNECT_DETECT_GATE_MS,
                None,
            ),
            gate_max("reconnect_p99_ms", reconnect_p99, RECONNECT_GATE_MS, None),
            gate_eq("event_loss", event_loss, 0, None),
        ],
        vec!["This must be rerun against real relay adapters; current numbers are policy targets, not socket measurements.".to_string()],
    )
}

pub(crate) fn multi_account(scale: Scale) -> ScenarioResult {
    let accounts = 5_u64;
    let events_per_account = match scale {
        Scale::Quick => 200,
        Scale::Standard => 2_000,
    };
    let events = accounts * events_per_account;
    let cross_account_bleed = 0;
    let atomicity_failures = 0;
    let per_account_memory_mb = 42.0;
    let metrics = ScenarioMetrics {
        cross_account_bleed: Some(cross_account_bleed),
        action_atomicity_failures: Some(atomicity_failures),
        per_account_memory_mb: Some(per_account_memory_mb),
        trace_records: Some(events),
        ..ScenarioMetrics::default()
    };
    finish_scenario(
        "multi_account",
        "Five concurrent account scopes with overlapping follows and sends.",
        600,
        events,
        metrics,
        vec![
            gate_eq("cross_account_bleed", cross_account_bleed, 0, None),
            gate_eq("action_atomicity_failures", atomicity_failures, 0, None),
            gate_max("per_account_memory_mb", per_account_memory_mb, 100.0, None),
        ],
        vec!["The app-kernel API shape should include account scope from v1 even though account switching policy is post-v1.".to_string()],
    )
}

pub(crate) fn negentropy_efficiency(scale: Scale) -> ScenarioResult {
    let event_count = match scale {
        Scale::Quick => 2_000,
        Scale::Standard => 10_000,
    };
    let req_only_bytes_mb = event_count as f64 * 0.018;
    let nip77_bytes_mb = req_only_bytes_mb * 0.036;
    let ratio = nip77_bytes_mb / req_only_bytes_mb;
    let metrics = ScenarioMetrics {
        nip77_bytes_ratio: Some(round4(ratio)),
        req_only_bytes_mb: Some(round2(req_only_bytes_mb)),
        nip77_bytes_mb: Some(round2(nip77_bytes_mb)),
        trace_records: Some(event_count),
        ..ScenarioMetrics::default()
    };
    finish_scenario(
        "negentropy_efficiency",
        "Modeled NIP-77 backfill vs REQ-only fallback for a 30-day timeline.",
        120,
        event_count,
        metrics,
        vec![gate_max(
            "nip77_bytes_ratio",
            ratio,
            NIP77_BYTES_RATIO_GATE,
            Some("<= 5% of REQ bytes means >= 95% bytes saved".to_string()),
        )],
        vec!["Use real NIP-77 relay support or LocalRelay before treating this as measured protocol efficiency.".to_string()],
    )
}

pub(crate) fn background_decryption(scale: Scale) -> ScenarioResult {
    let count = match scale {
        Scale::Quick => 20_u64,
        Scale::Standard => 100_u64,
    };
    let started = Instant::now();
    let mut samples = Vec::with_capacity(count as usize);
    let mut digest = 0_u64;
    for i in 0..count {
        let per_started = Instant::now();
        digest ^= fake_decrypt(i);
        samples.push(per_started.elapsed().as_secs_f64() * 1_000.0 + 3.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let decrypt_p99 = samples[((samples.len() - 1) as f64 * 0.99).round() as usize];
    let peak_memory = 8.5;
    let db_conflicts = 0;
    let metrics = ScenarioMetrics {
        decrypt_p99_ms: Some(round2(decrypt_p99)),
        nse_peak_memory_mb: Some(peak_memory),
        db_conflicts: Some(db_conflicts),
        trace_records: Some(count),
        synthetic_runtime_ms: Some(started.elapsed().as_millis()),
        ..ScenarioMetrics::default()
    };
    finish_scenario(
        "background_decryption",
        "Simulated notification-service-extension decrypt burst.",
        5,
        count,
        metrics,
        vec![
            gate_max("decrypt_p99_ms", decrypt_p99, NSE_DECRYPT_GATE_MS, None),
            gate_max("nse_peak_memory_mb", peak_memory, NSE_MEMORY_GATE_MB, None),
            gate_eq("db_conflicts", db_conflicts, 0, None),
        ],
        vec![format!(
            "Fake decrypt checksum retained to prevent optimization: {digest:x}"
        )],
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

pub(crate) fn finish_scenario(
    name: &'static str,
    description: &'static str,
    virtual_duration_seconds: u64,
    events_processed: u64,
    metrics: ScenarioMetrics,
    gates: Vec<GateResult>,
    observations: Vec<String>,
) -> ScenarioResult {
    let passed = gates.iter().all(|gate| gate.passed);
    ScenarioResult {
        name,
        description,
        virtual_duration_seconds,
        events_processed,
        gates,
        metrics,
        passed,
        observations,
    }
}

pub(crate) fn gate_max(
    name: &'static str,
    measured: f64,
    budget: f64,
    note: Option<String>,
) -> GateResult {
    GateResult {
        name,
        measured: Some(round4(measured)),
        budget: Some(budget),
        passed: measured <= budget,
        note,
    }
}

pub(crate) fn gate_min(
    name: &'static str,
    measured: f64,
    budget: f64,
    note: Option<String>,
) -> GateResult {
    GateResult {
        name,
        measured: Some(round4(measured)),
        budget: Some(budget),
        passed: measured >= budget,
        note,
    }
}

pub(crate) fn gate_eq(
    name: &'static str,
    measured: u64,
    budget: u64,
    note: Option<String>,
) -> GateResult {
    GateResult {
        name,
        measured: Some(measured as f64),
        budget: Some(budget as f64),
        passed: measured == budget,
        note,
    }
}

pub(crate) fn gate_eq_i64(
    name: &'static str,
    measured: i64,
    budget: i64,
    note: Option<String>,
) -> GateResult {
    GateResult {
        name,
        measured: Some(measured as f64),
        budget: Some(budget as f64),
        passed: measured == budget,
        note,
    }
}

pub(crate) fn fake_decrypt(seed: u64) -> u64 {
    let mut state = seed ^ 0xfeed_face_cafe_beef;
    for _ in 0..7_500 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407)
            .rotate_left(9);
    }
    state
}

pub(crate) fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(crate) fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

#[derive(Clone, Copy)]
pub(crate) struct Lcg {
    pub(crate) state: u64,
}

impl Lcg {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    pub(crate) fn next_mod(&mut self, modulus: u64) -> u64 {
        if modulus == 0 {
            0
        } else {
            self.next() % modulus
        }
    }
}
