//! Negative D9 fixture — injected time is allowed, and an explicit
//! reasoned allow can document a narrow residual.

struct Kernel {
    now_ms_value: u64,
    now_secs_value: u64,
}

impl Kernel {
    fn now_ms(&self) -> u64 {
        self.now_ms_value
    }

    fn now_secs(&self) -> u64 {
        self.now_secs_value
    }

    fn reducer_uses_injected_clock(&self) -> (u64, u64) {
        (self.now_ms(), self.now_secs())
    }

    fn caller_supplies_time(&mut self, now_ms: u64) {
        self.record(now_ms);
    }

    fn residual_with_issue(&self) {
        let _raw = std::time::SystemTime::now(); // doctrine-allow: D9 — fixture proves reasoned residual escape
    }

    fn record(&mut self, _now_ms: u64) {}
}
