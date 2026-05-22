//! Replay-mode scenario dispatcher and shared utilities.
//!
//! Per-scenario families:
//! - `timeline.rs` — cold_start, sustained_firehose, soak
//! - `other.rs` — profile_thrashing, relay_disconnect_storm, multi_account,
//!   background_decryption

mod other;
mod timeline;

pub(crate) use other::{
    background_decryption, multi_account, profile_thrashing, relay_disconnect_storm,
};
pub(crate) use timeline::{cold_start, soak, sustained_firehose};

use crate::config::Scale;
use crate::report::{GateResult, ScenarioMetrics, ScenarioResult};

pub(crate) fn run_scenario(name: &'static str, scale: Scale) -> ScenarioResult {
    match name {
        "cold_start" => cold_start(scale),
        "sustained_firehose" => sustained_firehose(scale),
        "profile_thrashing" => profile_thrashing(scale),
        "relay_disconnect_storm" => relay_disconnect_storm(scale),
        "multi_account" => multi_account(scale),
        "background_decryption" => background_decryption(scale),
        "soak" => soak(scale),
        _ => unreachable!("selected_scenarios validates names"),
    }
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
