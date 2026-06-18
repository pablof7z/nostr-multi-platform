//! Positive no_raw_tap fixture — must trigger a no_raw_tap finding.
//!
//! Production-shaped code (no `#[cfg(test)]` gate, not a `tests.rs`-shaped
//! filename) containing a banned raw-event-tap symbol.

pub struct LegacyIntegration {
    observer: RawEventObserver,
}

impl LegacyIntegration {
    pub fn setup(&mut self) {
        self.register_raw_event_observer(|_event| {});
    }
}
