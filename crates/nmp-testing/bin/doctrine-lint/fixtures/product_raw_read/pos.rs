//! Positive product_raw_read fixture.

pub fn product_shell_raw_reads(app: &NmpApp) {
    app.open_interest("{}".to_string(), "shell".to_string(), 0);
    app.register_snapshot_tick_observer(|| {});
}

pub struct NmpApp;

impl NmpApp {
    pub fn open_interest(&self, _filter: String, _consumer_id: String, _scope: u32) {}

    pub fn register_snapshot_tick_observer(&self, _f: impl Fn() + Send + Sync + 'static) {}
}
