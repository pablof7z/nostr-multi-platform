//! Real-relay soak runner — sustained multi-relay subscription churn.
//!
//! Proves three invariants over `NMP_SOAK_DURATION_SECS` (default 120,
//! clamped to `[10, 3600]`):
//!
//! 1. **Zero leaked subs** — every REQ id opened is later CLOSEd.
//! 2. **Bounded working set** — simultaneously-live sub ids never exceed
//!    `relays × 2` (honest deterministic bound; we do *not* sample RSS).
//! 3. **No panic** — a relay dying mid-soak is a recorded degradation, not a
//!    crash; only ZERO surviving relays is a SKIP, and a real leak is a loud
//!    FAIL.
//!
//! The report is written **before** any leak assertion fires, so even a real
//! FAIL leaves on-disk evidence in `docs/perf/real-relay/soak-<ts>.md`.
//!
//! ```bash
//! NMP_SOAK_DURATION_SECS=120 \
//!   cargo test -p nmp-testing --test real_relay_soak -- --ignored --nocapture
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;

#[path = "soak/mod.rs"]
mod soak;

use common::Verdict;

#[test]
#[ignore = "real-relay soak (run with --ignored)"]
fn multi_relay_subscription_soak() {
    let result = soak::run_soak();

    // Evidence first: persist UNCONDITIONALLY so even a FAIL/SKIP leaves a
    // greppable artifact on disk before any assertion can panic.
    soak::persist_report(&result);

    println!(
        "[soak] verdict={} duration={}s relays={} req_opened={} req_closed={} \
         events_seen={} max_live_subs={} ceiling={} windows={} errors={}",
        result.verdict.as_str(),
        result.duration_s,
        result.relays.len(),
        result.req_opened,
        result.req_closed,
        result.events_seen,
        result.max_live_subs,
        result.ceiling,
        result.windows,
        result.errors.len(),
    );
    for (url, n) in &result.per_relay {
        println!("[soak]   {url}: {n} EVENT frames");
    }
    for e in &result.errors {
        eprintln!("[soak] degradation: {e}");
    }

    match result.verdict {
        Verdict::Skip => {
            // Zero reachable relays: honest SKIP, never a fabricated green.
            eprintln!(
                "SKIP: no relay survived the soak — wrote finding to \
                 docs/perf/real-relay/soak-{}.md",
                result.started_at
            );
        }
        Verdict::Pass => {
            // Working-set bound is part of PASS — assert it explicitly too.
            assert!(
                result.max_live_subs <= result.ceiling,
                "working-set bound exceeded: {} live subs > ceiling {}",
                result.max_live_subs,
                result.ceiling
            );
            assert!(
                result.leaked.is_empty(),
                "internal invariant: PASS with leaked subs {:?}",
                result.leaked
            );
            assert_eq!(
                result.req_opened, result.req_closed,
                "internal invariant: PASS but opened {} != closed {}",
                result.req_opened, result.req_closed
            );
            println!(
                "[soak] PASS — {} subs opened+closed cleanly, max live {}/{} \
                 over {}s",
                result.req_opened, result.max_live_subs, result.ceiling, result.duration_s
            );
        }
        Verdict::Fail => {
            // A real leak MUST be loud. Report already persisted above.
            panic!(
                "LEAK: {} sub id(s) opened but never CLOSEd (opened={}, \
                 closed={}, max_live={}, ceiling={}): {:?}",
                result.leaked.len(),
                result.req_opened,
                result.req_closed,
                result.max_live_subs,
                result.ceiling,
                result.leaked
            );
        }
    }
}
