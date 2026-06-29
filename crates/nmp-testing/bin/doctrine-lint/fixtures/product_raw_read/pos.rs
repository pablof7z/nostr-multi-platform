//! Positive product_raw_read fixture.

pub fn product_shell_raw_reads(app: &NmpApp) {
    app.open_interest("{}".to_string(), "shell".to_string(), 0);
    app.close_interest("shell");
    app.open_observed_projection("timeline");
    app.register_snapshot_tick_observer(|| {});
    let _sink: ObservedProjectionSink = ObservedProjectionSink;
    let _projection: ObservedProjection = ObservedProjection;
}

pub struct NmpApp;

impl NmpApp {
    pub fn open_interest(&self, _filter: String, _consumer_id: String, _scope: u32) {}

    pub fn close_interest(&self, _interest_id: &str) {}

    pub fn open_observed_projection(&self, _name: &str) {}

    pub fn register_snapshot_tick_observer(&self, _f: impl Fn() + Send + Sync + 'static) {}
}

pub struct ObservedProjectionSink;

pub struct ObservedProjection;
