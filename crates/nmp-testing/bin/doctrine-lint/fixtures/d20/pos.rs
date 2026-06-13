// D20 positive fixture — raw std::time on a (notionally) wasm-reachable path.
// Each banned line below must fire a D20 finding.

// (1) Grouped import naming Instant — the headline case a single-needle
// `std::time::Instant` check would miss.
use std::time::{Duration, Instant};

// (2) Grouped import naming SystemTime.
use std::time::{SystemTime, UNIX_EPOCH};

fn measure() -> Duration {
    // (3) Inline fully-qualified Instant::now() call site.
    let start = std::time::Instant::now();
    do_work();
    start.elapsed()
}

fn wall_clock() -> u64 {
    // (4) Inline fully-qualified SystemTime::now() call site.
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn do_work() {}
