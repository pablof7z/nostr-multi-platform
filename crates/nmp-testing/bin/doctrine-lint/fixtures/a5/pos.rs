//! A5 positive fixture — contains in-repo calls to `register_raw_event_observer`
//! in production (non-definition, non-test) code. The lint must flag each
//! occurrence.

/// This represents a hypothetical state-derivation consumer that
/// incorrectly registers the raw tap instead of using `register_ingest_parser`.
pub fn install_bad_observer(app: &dyn std::any::Any) {
    // A5 violation: state is derived from the raw tap instead of IngestParser.
    // Should be: app.register_ingest_parser(filter, parser)
    let _id = get_app().register_raw_event_observer(all_kinds_filter(), bad_observer());
}

fn get_app() -> FakeApp {
    FakeApp
}

fn all_kinds_filter() -> &'static str {
    "{}"
}

fn bad_observer() -> impl Fn(&str) {
    |_| {}
}

struct FakeApp;

impl FakeApp {
    fn register_raw_event_observer(
        &self,
        _filter: &str,
        _observer: impl Fn(&str),
    ) -> u64 {
        0
    }
}
