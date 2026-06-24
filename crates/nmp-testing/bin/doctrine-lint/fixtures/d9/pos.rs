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

    fn registers_claim_with_raw_argument(&mut self) {
        self.register_claim_expansion("event", Instant::now());
    }

    fn captures_local_then_feeds_policy(&mut self) {
        let now = Instant::now();
        self.ingest_with_time(now);
    }

    fn updates_transport_info_with_raw_argument(&mut self) {
        self.set_info("wss://relay.example", Instant::now());
    }

    fn hidden_epoch_helper(&mut self) {
        let _now_ms = now_epoch_ms();
    }

    fn contacts_deadline(&mut self, _deadline: Instant) {}

    fn register_claim_expansion(&mut self, _event: &str, _now: Instant) {}

    fn ingest_with_time(&mut self, _now: Instant) {}

    fn set_info(&mut self, _relay_url: &str, _now: Instant) {}
}

fn now_epoch_ms() -> u64 {
    0
}
