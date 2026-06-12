//! A5 negative fixture — these shapes must NOT be flagged by rule A5.

/// Using `register_ingest_parser` is the correct state-derivation seam — not a violation.
pub fn install_dm_inbox_parser(app: &mut FakeApp) {
    app.replace_ingest_parser("nip17.dm_inbox", dm_inbox_parser());
}

/// Using `register_ingest_parser` for Marmot — correct seam, not flagged.
pub fn install_marmot_parser(app: &mut FakeApp) {
    app.register_ingest_parser(all_filter(), marmot_parser());
}

/// A doc comment mentioning `register_raw_event_observer` is not a call — exempt.
///
/// The verbatim-forwarding tap (`register_raw_event_observer`) is for external
/// store mirrors only. All in-repo state derivation uses `register_ingest_parser`.
pub fn documented_correctly() {}

/// Code that merely reads the result of `register_raw_event_observer` in a
/// string literal for doc/test purposes is not a production call.
pub fn string_reference() -> &'static str {
    "call register_ingest_parser instead of register_raw_event_observer"
}

pub struct FakeApp;

impl FakeApp {
    pub fn register_ingest_parser(
        &mut self,
        _filter: &str,
        _parser: impl Fn(&str),
    ) -> u64 {
        0
    }

    pub fn replace_ingest_parser(
        &mut self,
        _slot_key: &str,
        _parser: impl Fn(&str),
    ) {
    }
}

fn all_filter() -> &'static str {
    "{}"
}

fn dm_inbox_parser() -> impl Fn(&str) {
    |_| {}
}

fn marmot_parser() -> impl Fn(&str) {
    |_| {}
}

#[cfg(test)]
mod tests {
    use super::*;

    impl FakeApp {
        // This definition is inside cfg(test) — A5 exempts test bodies.
        // It exposes the raw tap ONLY for test-level seam verification.
        pub fn register_raw_event_observer(
            &mut self,
            _filter: &str,
            _observer: impl Fn(&str),
        ) -> u64 {
            0
        }

        pub fn free_raw_observer(&mut self, _id: u64) {}
    }

    /// Inside a cfg(test) block: `register_raw_event_observer` calls are exempt
    /// because test code is allowed to exercise the seam directly.
    #[test]
    fn test_tap_exempted_in_test_cfg() {
        let mut app = FakeApp;
        // This call is inside cfg(test) — rule A5 exempts test bodies.
        let _id = app.register_raw_event_observer("{}", |_ev: &str| {});
        app.free_raw_observer(_id);
    }
}
