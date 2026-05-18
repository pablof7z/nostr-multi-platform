//! Live cold-start scenario.
//!
//! Drives the real kernel against `wss://relay.primal.net` and
//! `wss://purplepag.es`.  Measures:
//!
//! - time-to-first-item (`visible_items >= 1`)
//! - time-to-filled-timeline (`visible_items >= 200`)
//!
//! Gates (M1 exit):
//! - first-item ≤ 800 ms (p99 on developer hardware)
//! - filled-timeline ≤ 5 000 ms

use super::{drain, metric, round2, wait_connected, wait_update, Scenario};
use crate::scenarios::{finish_scenario, gate_max};
use crate::report::ScenarioMetrics;
use nmp_core::testing::{spawn_actor, ActorCommand};
use std::time::{Duration, Instant};

const FIRST_ITEM_GATE_MS: f64 = 800.0;
const FILLED_TIMELINE_GATE_MS: f64 = 5_000.0;
const FIRST_ITEM_TIMEOUT: Duration = Duration::from_secs(30);
const FILLED_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) fn cold_start() -> Scenario {
    let wall_start = Instant::now();
    let (tx, rx) = spawn_actor();
    let _ = tx.send(ActorCommand::Start {
        visible_limit: 200,
        emit_hz: 4,
    });

    let connected = wait_connected(&rx);

    let mut first_item_ms: Option<f64> = None;
    let mut filled_ms: Option<f64> = None;
    let mut peak_visible: usize = 0;
    let mut error: Option<String> = None;

    if !connected {
        error = Some(format!(
            "relay did not connect within {}s",
            super::WARMUP_TIMEOUT.as_secs()
        ));
    } else {
        let measure_start = Instant::now();
        let first_deadline = measure_start + FIRST_ITEM_TIMEOUT;
        let filled_deadline = measure_start + FILLED_TIMEOUT;

        loop {
            let deadline = if first_item_ms.is_none() {
                first_deadline
            } else {
                filled_deadline
            };
            let Some(update) = wait_update(&rx, deadline) else {
                if first_item_ms.is_none() {
                    error = Some(format!(
                        "no timeline item within {}s",
                        FIRST_ITEM_TIMEOUT.as_secs()
                    ));
                }
                break;
            };
            let visible = metric(&update, "visible_items").unwrap_or(0.0) as usize;
            peak_visible = peak_visible.max(visible);

            if first_item_ms.is_none() && visible >= 1 {
                first_item_ms = Some(measure_start.elapsed().as_secs_f64() * 1_000.0);
            }
            if first_item_ms.is_some() && filled_ms.is_none() && visible >= 200 {
                filled_ms = Some(measure_start.elapsed().as_secs_f64() * 1_000.0);
                break;
            }
            if measure_start.elapsed() >= FILLED_TIMEOUT {
                break;
            }
        }
    }

    let _ = tx.send(ActorCommand::Shutdown);
    let _ = drain(&rx);

    let first_ms = first_item_ms.unwrap_or(FIRST_ITEM_TIMEOUT.as_millis() as f64);
    let full_ms = filled_ms.unwrap_or(FILLED_TIMEOUT.as_millis() as f64);

    let metrics = ScenarioMetrics {
        first_item_ms: Some(round2(first_ms)),
        filled_timeline_ms: Some(round2(full_ms)),
        peak_memory_mb: None,
        synthetic_runtime_ms: Some(wall_start.elapsed().as_millis()),
        trace_records: Some(peak_visible as u64),
        ..ScenarioMetrics::default()
    };

    let mut obs = vec![
        format!("peak_visible_items={peak_visible}"),
        "hardware=M3_Max developer machine".to_string(),
        "relays=wss://relay.primal.net + wss://purplepag.es".to_string(),
    ];
    if let Some(err) = &error {
        obs.push(format!("error={err}"));
    }

    finish_scenario(
        "cold_start",
        "Live: cold-start against primal + purplepag.es with real WebSocket I/O.",
        FILLED_TIMEOUT.as_secs(),
        peak_visible as u64,
        metrics,
        vec![
            gate_max("first_item_ms", first_ms, FIRST_ITEM_GATE_MS, None),
            gate_max("filled_timeline_ms", full_ms, FILLED_TIMELINE_GATE_MS, None),
        ],
        obs,
    )
}
