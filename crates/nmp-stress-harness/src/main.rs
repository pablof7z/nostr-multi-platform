//! NMP event-flow stress harness — a REAL, RUNNABLE throwaway binary that
//! validates the landed event-flow architecture against real data.
//!
//! This is NOT a `#[test]` suite (so `cargo test` / CI never runs it). Run it
//! manually:
//!
//! ```text
//! cargo run -p nmp-stress-harness
//! ```
//!
//! It prints a `PASS` / `FAIL` / `SKIP` line per scenario and a summary, and
//! exits nonzero if any scenario FAILS. A `FAIL` on a landed-behavior scenario
//! is a genuine finding about master, not a harness bug — the report shows the
//! real output.
//!
//! ## What is real here
//! - Real generated `nostr::Keys` + `add_signer(LocalNsec …)`.
//! - Real Schnorr signing through the publish engine (`dispatch_action`).
//! - Real `nmp_nip59::gift_wrap_local` for DMs.
//! - An embedded in-process fixture relay (`src/relay.rs`) — events flow
//!   through the REAL relay worker → `handle_event` → `verify_and_persist`
//!   chokepoint (ADR-0057). The catalog noted no in-process mock relay exists
//!   in `nmp-testing`; this is the minimal one.
//! - An injected deterministic `MonotonicSecondClock` for D9 / GC / NIP-40.
//! - Persistent `storage_path` (tempdir) for cold-restart scenarios.
//!
//! ## Coverage staging (per the merged Opus+codex catalog)
//! - LANDED now (PR1 chokepoint, Workstream C publish-policy, PR2 profiles):
//!   implemented and asserted.
//! - NOT yet landed (PR3 contacts→parser, Workstream B acquisition-one-door,
//!   Workstream F doctrine gates): printed as `SKIP (pending PRn)` so the
//!   coverage report stays honest.

mod harness;
mod relay;
mod scenarios;

use std::time::Duration;

/// Outcome of a single scenario.
pub enum Outcome {
    Pass,
    Fail(String),
    /// Skipped because the behavior it validates has not landed yet.
    Skip(String),
}

pub struct ScenarioResult {
    pub id: &'static str,
    pub title: &'static str,
    /// Which transport the scenario drove: the fixture relay (real ingest
    /// chokepoint) or the kernel-injection seam, or local publish only.
    pub driver: &'static str,
    pub outcome: Outcome,
}

/// Default per-scenario wait budget for async actor / relay round-trips.
pub const WAIT: Duration = Duration::from_secs(5);

fn main() {
    println!("=== NMP event-flow stress harness ===");
    println!("Real keys + Schnorr · embedded fixture relay · injected clock · persistent store\n");

    let results = scenarios::run_all();

    let mut pass = 0;
    let mut fail = 0;
    let mut skip = 0;
    println!("\n--- Results ---");
    for r in &results {
        match &r.outcome {
            Outcome::Pass => {
                pass += 1;
                println!("PASS  {:<10} [{}] {}", r.id, r.driver, r.title);
            }
            Outcome::Fail(why) => {
                fail += 1;
                println!("FAIL  {:<10} [{}] {}\n        ↳ {}", r.id, r.driver, r.title, why);
            }
            Outcome::Skip(why) => {
                skip += 1;
                println!("SKIP  {:<10} [{}] {} — {}", r.id, r.driver, r.title, why);
            }
        }
    }

    println!(
        "\n--- Summary --- {} PASS, {} FAIL, {} SKIP, {} total",
        pass,
        fail,
        skip,
        results.len()
    );
    if fail > 0 {
        println!(
            "\n{} scenario(s) FAILED. A failure on a landed-behavior scenario is a real \
             finding about master — see the ↳ evidence above.",
            fail
        );
        std::process::exit(1);
    }
    println!("\nAll non-skipped scenarios PASSED.");
}
