use super::BrowserRuntimeHandle;

impl BrowserRuntimeHandle {
    /// Spawn relay drivers from `bootstrap` (wasm32: opens WebSockets; native:
    /// no-op). Called once from `from_builder_inner` with the bootstrap list
    /// captured before it was consumed into the kernel.
    ///
    /// Any socket-budget-exceeded or spawn-failed events from bootstrap are
    /// parked in `pending_startup_events` and surfaced on the first `pump()`
    /// (D6 -- a bad bootstrap relay is never silently dropped).
    pub(super) fn spawn_relay_bootstrap(&mut self, bootstrap: &[(String, String)]) {
        #[cfg(target_arch = "wasm32")]
        {
            let events = self.runtime.relay_pool.spawn_bootstrap(bootstrap);
            self.runtime.pending_startup_events.extend(events);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = bootstrap;
    }
}
