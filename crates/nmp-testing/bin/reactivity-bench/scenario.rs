use crate::allocator::allocation_snapshot;
use crate::config::{
    ScenarioConfig, ViewMix, ALLOCATION_WARMUP_EVENTS, CANDIDATES_PER_DELTA_GATE,
    DELTAS_PER_VIEW_SEC_GATE, DELTA_FLUSH_THRESHOLD, EMIT_HZ_GATE, FALSE_WAKEUP_RATE_GATE,
    LOOKUP_P99_GATE_NS, RECOMPUTE_P99_GATE_NS, WORKING_SET_MEMORY_GATE_BYTES,
};
use crate::domain::DeltaBuffer;
use crate::report::{GateResult, ScenarioResult};
use crate::rng::{seed_for, Lcg};
use crate::world::BenchWorld;
use std::time::Instant;

pub(crate) fn run_scenario(config: ScenarioConfig) -> ScenarioResult {
    let started = Instant::now();
    let mut rng = Lcg::new(seed_for(config.name));
    let mut bench = BenchWorld::new(
        config.author_count,
        config.cached_events,
        config.hot_event_limit,
    );
    bench.prepopulate(&mut rng);
    bench.open_views(config.view_mix);

    let mut lookup_samples = Vec::with_capacity(config.replay_events);
    let mut recompute_samples = Vec::with_capacity(config.replay_events);
    let mut delta_buffer = DeltaBuffer::new(
        bench.views.len(),
        config.replay_events.min(DELTA_FLUSH_THRESHOLD),
    );
    let mut simulated_now_ns = 0_u64;
    let event_interval_ns = 1_000_000_000_u64 / config.event_rate_per_sec.max(1);
    let mut interested_events = 0_u64;
    let mut candidate_view_hits = 0_u64;
    let mut false_wakeups = 0_u64;
    let mut raw_deltas = 0_u64;
    let mut measured_allocations = 0_u64;
    let mut measured_allocated_bytes = 0_u64;
    let mut measured_events = 0_usize;
    let mut steady_state_peak_heap_bytes = allocation_snapshot().peak_heap_bytes;

    for index in 0..config.replay_events {
        let event = bench.next_event(config.stream, index, &mut rng);
        let measure_allocations = index >= ALLOCATION_WARMUP_EVENTS;
        let alloc_before = measure_allocations.then(allocation_snapshot);

        let lookup_started = Instant::now();
        bench.lookup_into(&event);
        lookup_samples.push(lookup_started.elapsed().as_nanos());

        let hit_count = bench.hit_count();
        if hit_count > 0 {
            interested_events += 1;
        }
        candidate_view_hits += hit_count as u64;

        let recompute_started = Instant::now();
        let processed = bench.apply_event(&event, &mut delta_buffer);
        recompute_samples.push(recompute_started.elapsed().as_nanos());
        false_wakeups += hit_count.saturating_sub(processed.raw_delta_count) as u64;

        raw_deltas += processed.raw_delta_count as u64;
        simulated_now_ns = simulated_now_ns.saturating_add(event_interval_ns);
        delta_buffer.maybe_flush(simulated_now_ns, false);

        if let Some(before) = alloc_before {
            let after = allocation_snapshot();
            measured_allocations += after.allocations.saturating_sub(before.allocations);
            measured_allocated_bytes +=
                after.allocated_bytes.saturating_sub(before.allocated_bytes);
            steady_state_peak_heap_bytes = steady_state_peak_heap_bytes.max(after.peak_heap_bytes);
            measured_events += 1;
        }
    }

    delta_buffer.maybe_flush(simulated_now_ns, true);

    lookup_samples.sort_unstable();
    recompute_samples.sort_unstable();

    let elapsed = started.elapsed().as_millis();
    let simulated_seconds = config.replay_events as f64 / config.event_rate_per_sec.max(1) as f64;
    let max_batch_hz = if simulated_seconds == 0.0 {
        0.0
    } else {
        delta_buffer.batches as f64 / simulated_seconds
    };
    let coalesced_deltas_per_sec = if simulated_seconds == 0.0 {
        0.0
    } else {
        delta_buffer.coalesced_delta_count as f64 / simulated_seconds
    };
    let max_deltas_per_view_per_sec = if simulated_seconds == 0.0 {
        0.0
    } else {
        delta_buffer
            .per_view_coalesced
            .iter()
            .copied()
            .max()
            .unwrap_or(0) as f64
            / simulated_seconds
    };
    let estimated_working_set_memory_bytes = bench.estimated_working_set_memory_bytes();
    let candidates_per_delta = if raw_deltas == 0 {
        0.0
    } else {
        round2(candidate_view_hits as f64 / raw_deltas as f64)
    };
    let false_wakeup_rate = if candidate_view_hits == 0 {
        0.0
    } else {
        round2(false_wakeups as f64 / candidate_view_hits as f64)
    };
    let allocations_per_event = if measured_events == 0 {
        None
    } else {
        Some(round4(measured_allocations as f64 / measured_events as f64))
    };

    let lookup_p99_ns = percentile(&lookup_samples, 0.99);
    let recompute_p99_ns = percentile(&recompute_samples, 0.99);
    let mut gates = vec![
        GateResult {
            name: "lookup_p99_ns",
            measured: Some(lookup_p99_ns as f64),
            budget: Some(LOOKUP_P99_GATE_NS as f64),
            passed: lookup_p99_ns <= LOOKUP_P99_GATE_NS,
            note: None,
        },
        GateResult {
            name: "recompute_p99_ns",
            measured: Some(recompute_p99_ns as f64),
            budget: Some(RECOMPUTE_P99_GATE_NS as f64),
            passed: recompute_p99_ns <= RECOMPUTE_P99_GATE_NS,
            note: None,
        },
        GateResult {
            name: "view_batch_hz",
            measured: Some(max_batch_hz),
            budget: Some(EMIT_HZ_GATE),
            passed: max_batch_hz <= EMIT_HZ_GATE,
            note: None,
        },
        GateResult {
            name: "max_deltas_per_view_per_sec",
            measured: Some(max_deltas_per_view_per_sec),
            budget: Some(DELTAS_PER_VIEW_SEC_GATE),
            passed: max_deltas_per_view_per_sec <= DELTAS_PER_VIEW_SEC_GATE,
            note: Some("ADR-0002 gate: per-view, not absolute".to_string()),
        },
        GateResult {
            name: "false_wakeup_rate",
            measured: Some(false_wakeup_rate),
            budget: Some(FALSE_WAKEUP_RATE_GATE),
            passed: false_wakeup_rate <= FALSE_WAKEUP_RATE_GATE,
            note: Some("ADR-0001 gate".to_string()),
        },
        GateResult {
            name: "candidates_per_delta",
            measured: Some(candidates_per_delta),
            budget: Some(CANDIDATES_PER_DELTA_GATE),
            passed: candidates_per_delta <= CANDIDATES_PER_DELTA_GATE,
            note: Some("candidate hits divided by raw pre-coalesced deltas".to_string()),
        },
        GateResult {
            name: "estimated_working_set_memory_bytes",
            measured: Some(estimated_working_set_memory_bytes as f64),
            budget: Some(WORKING_SET_MEMORY_GATE_BYTES as f64),
            passed: estimated_working_set_memory_bytes <= WORKING_SET_MEMORY_GATE_BYTES,
            note: Some(format!(
                "hot_event_limit={}; cached_events={} disk-modeled",
                config.hot_event_limit, config.cached_events
            )),
        },
    ];

    gates.push(if measured_events == 0 {
        GateResult {
            name: "steady_state_allocations",
            measured: None,
            budget: Some(0.0),
            passed: true,
            note: Some(format!(
                "not applicable: replay_events={} does not exceed {}-event warmup",
                config.replay_events, ALLOCATION_WARMUP_EVENTS
            )),
        }
    } else {
        GateResult {
            name: "steady_state_allocations",
            measured: Some(measured_allocations as f64),
            budget: Some(0.0),
            passed: measured_allocations == 0,
            note: Some(format!(
                "measured {} events after warmup; {} bytes allocated",
                measured_events, measured_allocated_bytes
            )),
        }
    });

    let passed = gates.iter().all(|gate| gate.passed);
    let notes = scenario_notes(
        config,
        &bench,
        raw_deltas,
        delta_buffer.coalesced_delta_count,
        max_deltas_per_view_per_sec,
    );

    ScenarioResult {
        name: config.name.to_string(),
        cached_events: config.cached_events,
        hot_events_resident: bench.hot_events.len(),
        replay_events: config.replay_events,
        event_rate_per_sec: config.event_rate_per_sec,
        open_views: bench.views.len(),
        interested_events,
        candidate_view_hits,
        false_wakeups,
        candidates_per_delta,
        false_wakeup_rate,
        raw_deltas,
        coalesced_deltas: delta_buffer.coalesced_delta_count,
        batches: delta_buffer.batches,
        duration_ms: elapsed,
        lookup_p50_ns: percentile(&lookup_samples, 0.50),
        lookup_p99_ns,
        recompute_p50_ns: percentile(&recompute_samples, 0.50),
        recompute_p99_ns,
        max_batch_hz,
        coalesced_deltas_per_sec,
        max_deltas_per_view_per_sec,
        estimated_working_set_memory_bytes,
        steady_state_allocations_measured: measured_events > 0,
        steady_state_events_measured: measured_events,
        steady_state_allocations: measured_allocations,
        steady_state_allocated_bytes: measured_allocated_bytes,
        steady_state_allocations_per_event: allocations_per_event,
        steady_state_peak_heap_bytes,
        gates,
        passed,
        notes,
    }
}

pub(crate) fn scenario_notes(
    config: ScenarioConfig,
    bench: &BenchWorld,
    raw_deltas: u64,
    coalesced_deltas: u64,
    max_deltas_per_view_per_sec: f64,
) -> Vec<String> {
    let mut notes = Vec::new();
    match config.view_mix {
        ViewMix::HashtagFirehose => {
            notes.push(format!(
                "catch_all views considered on every insert: {}",
                bench.index.catch_all.len()
            ));
        }
        ViewMix::ProfileFanout => {
            notes.push(format!(
                "shared author kind:0 fan-out touched {} timeline views before flush coalescing",
                bench.profile_fanout_hits
            ));
        }
        ViewMix::ThreadBlowup => {
            notes.push(format!(
                "thread state after replay: replies={}, reactions={}",
                bench.thread_reply_count, bench.thread_reaction_count
            ));
        }
        ViewMix::AccountSwitch => {
            notes.push(format!(
                "account switch path re-registered views; insert-path coalesced to {} deltas",
                coalesced_deltas
            ));
        }
        ViewMix::WorkingSet100Views => {
            notes.push(format!(
                "working-set gate scenario: cached_events={}, hot_events={}, open_views={}",
                config.cached_events,
                bench.hot_events.len(),
                bench.views.len()
            ));
        }
        _ => {}
    }
    if raw_deltas != coalesced_deltas {
        notes.push(format!(
            "within-view coalescing reduced raw deltas {} -> {} ({:.2}/view/sec max)",
            raw_deltas, coalesced_deltas, max_deltas_per_view_per_sec
        ));
    }
    notes
}

pub(crate) fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(crate) fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

pub(crate) fn percentile(sorted: &[u128], percentile: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}
