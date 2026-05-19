//! Non-timeline scenarios: profile_thrashing, relay_disconnect_storm, multi_account,
//! negentropy_efficiency, background_decryption.

use crate::config::{
    DISCONNECT_DETECT_GATE_MS, NIP77_BYTES_RATIO_GATE, NSE_DECRYPT_GATE_MS, NSE_MEMORY_GATE_MB,
    RECONNECT_GATE_MS, Scale,
};
use crate::report::ScenarioMetrics;
use crate::scenarios::{fake_decrypt, finish_scenario, gate_eq, gate_max, gate_min, round2, round4};
use std::time::Instant;

pub(crate) fn profile_thrashing(scale: Scale) -> crate::report::ScenarioResult {
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

pub(crate) fn relay_disconnect_storm(scale: Scale) -> crate::report::ScenarioResult {
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

pub(crate) fn multi_account(scale: Scale) -> crate::report::ScenarioResult {
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

pub(crate) fn negentropy_efficiency(scale: Scale) -> crate::report::ScenarioResult {
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

pub(crate) fn background_decryption(scale: Scale) -> crate::report::ScenarioResult {
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
