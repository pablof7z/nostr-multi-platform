// D20 negative fixture — wasm-safe time usage. None of these lines may fire.

// Correct: import the wasm-safe shim, not std::time, for Instant/SystemTime.
use crate::time::Instant;
use crate::time::{SystemTime, UNIX_EPOCH};

// Correct: Duration is the same type on both targets, so importing it
// directly from std::time is allowed (D20 must NOT flag a Duration-only import).
use std::time::Duration;

fn measure() -> Duration {
    // Correct: route now() through the shim.
    let start = Instant::now();
    do_work();
    start.elapsed()
}

fn wall_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn do_work() {}

#[cfg(test)]
mod tests {
    // Test code may use std::time directly — tests never run on wasm32.
    use std::time::{Duration, Instant};

    #[test]
    fn t() {
        let _ = std::time::Instant::now();
        let _ = std::time::SystemTime::now();
        let _: Option<Instant> = None;
        let _: Option<Duration> = None;
    }
}
