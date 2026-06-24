//! Positive D9 fixture — raw kernel-policy time reads must fire.

use std::time::{Duration, Instant, SystemTime};

struct Kernel;

impl Kernel {
    fn reducer_reads_wall_clock(&mut self) {
        let _now = SystemTime::now();
    }

    fn reducer_sets_deadline(&mut self) {
        self.contacts_deadline(Instant::now() + Duration::from_secs(3));
    }

    fn hidden_epoch_helper(&mut self) {
        let _now_ms = now_epoch_ms();
    }

    fn contacts_deadline(&mut self, _deadline: Instant) {}
}

fn now_epoch_ms() -> u64 {
    0
}
