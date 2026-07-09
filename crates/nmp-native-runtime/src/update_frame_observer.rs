//! #3127 — additive multicast update-frame observer registry.
//!
//! Generalizes the `IdentityChangeRegistrar` / `ConfiguredRelaysChangeRegistrar`
//! pattern (`app_struct.rs`) to fire UNCONDITIONALLY on every emitted update
//! frame, rather than only when a diffed value changes. Lives alongside the
//! single-owner `update_listener` slot (`passive_start.rs`) — it does not
//! replace it, and the shell (TUI/Swift) keeps that slot for frame delivery
//! to the UI. A logic-layer library that needs a reactive "re-check" signal
//! (e.g. `nmp-app-29er` driving a `KeyedReadCollection` reconcile, #3115)
//! registers here instead of stealing the shell's slot.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use crate::app_struct::NmpApp;

/// Handle returned by [`NmpApp::register_update_frame_observer`], passed back
/// to [`NmpApp::unregister_update_frame_observer`] to revoke a registration.
pub type UpdateFrameObserverId = u64;

type UpdateFrameObserverCallback = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub(crate) struct UpdateFrameObserverRegistration {
    id: UpdateFrameObserverId,
    callback: UpdateFrameObserverCallback,
}

pub(crate) type UpdateFrameObserverSlot = Arc<Mutex<Vec<UpdateFrameObserverRegistration>>>;

pub(crate) fn new_update_frame_observer_slot() -> UpdateFrameObserverSlot {
    Arc::new(Mutex::new(Vec::new()))
}

pub(crate) fn unregister_update_frame_observer(
    observers: &UpdateFrameObserverSlot,
    id: UpdateFrameObserverId,
) {
    if let Ok(mut registrations) = observers.lock() {
        registrations.retain(|registration| registration.id != id);
    }
}

/// Fire every registered observer for one emitted update frame.
///
/// Runs unconditionally, on EVERY frame — unlike `notify_identity_change_observers`
/// / `notify_configured_relays_change_observers`, there is no diffing against
/// a previous value; see the doc comment on
/// [`NmpApp::register_update_frame_observer`] for the "re-check" contract
/// this implies for callers.
///
/// Snapshots the callback list and releases the registry lock BEFORE invoking
/// any callback, so a callback that calls back into `NmpApp` (e.g. to open a
/// read or drive a reconcile, which may itself call
/// `register_update_frame_observer` / `unregister_update_frame_observer`)
/// cannot deadlock against this registry's own lock — the same re-entrancy
/// discipline #3078/#3080 established for the snapshot-projection registry.
pub(crate) fn notify_update_frame_observers(observers: &UpdateFrameObserverSlot) {
    let callbacks: Vec<UpdateFrameObserverCallback> = observers
        .lock()
        .map(|guard| {
            guard
                .iter()
                .map(|registration| Arc::clone(&registration.callback))
                .collect()
        })
        .unwrap_or_default();
    for callback in callbacks {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback()));
    }
}

impl NmpApp {
    /// Register a multicast callback that fires on EVERY emitted update
    /// frame.
    ///
    /// Unlike [`Self::set_update_listener`] (a SINGLE-owner slot the shell
    /// takes for frame delivery to the UI), this registry is additive: any
    /// number of consumers may register a callback here without disturbing
    /// the shell's slot or each other's registrations.
    ///
    /// The callback runs on the update-listener thread — the same
    /// off-actor-thread `set_update_listener`'s callback runs on. It is
    /// therefore safe for a registered callback to call back into `NmpApp`
    /// (e.g. `open_read`, a `KeyedReadCollection` reconcile, `resolve_ref`):
    /// those calls defer onto the actor's command queue rather than
    /// executing synchronously, so no deadlock is possible (#3078/#3080).
    ///
    /// Fires unconditionally on every emitted frame — there is no diffing
    /// against a previous value. Treat this as a "something may have
    /// changed, re-check" signal: the downstream reconcile a callback drives
    /// is expected to be idempotent and a no-op when nothing actually
    /// changed.
    ///
    /// Returns an [`UpdateFrameObserverId`] for
    /// [`Self::unregister_update_frame_observer`].
    pub fn register_update_frame_observer<F>(&self, callback: F) -> UpdateFrameObserverId
    where
        F: Fn() + Send + Sync + 'static,
    {
        let id = self
            .next_update_frame_observer_id
            .fetch_add(1, Ordering::Relaxed);
        if let Ok(mut observers) = self.update_frame_observers.lock() {
            observers.push(UpdateFrameObserverRegistration {
                id,
                callback: Arc::new(callback),
            });
        }
        id
    }

    /// Revoke a registration made by
    /// [`Self::register_update_frame_observer`]. Idempotent for unknown ids.
    pub fn unregister_update_frame_observer(&self, id: UpdateFrameObserverId) {
        unregister_update_frame_observer(&self.update_frame_observers, id);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use nmp_ownership::DynamicProjectionKey;
    use nmp_read_session::ReadHost;

    /// (a) — every registered observer fires on the same emitted frame.
    #[test]
    fn multiple_observers_all_fire_on_frame_emission() {
        let app = crate::new_app();
        app.start_runtime(50, 30);
        assert!(app.wait_barrier_for_test(Duration::from_secs(5)));

        let (tx1, rx1) = mpsc::channel::<()>();
        let (tx2, rx2) = mpsc::channel::<()>();
        app.register_update_frame_observer(move || {
            let _ = tx1.send(());
        });
        app.register_update_frame_observer(move || {
            let _ = tx2.send(());
        });

        app.add_relay("wss://frameobs-a.example".to_string(), "read".to_string());

        assert!(
            rx1.recv_timeout(Duration::from_secs(5)).is_ok(),
            "first observer must fire"
        );
        assert!(
            rx2.recv_timeout(Duration::from_secs(5)).is_ok(),
            "second observer must fire"
        );

        app.stop_runtime();
    }

    /// (b) — unregistering one observer leaves the others (and future
    /// frames) unaffected.
    #[test]
    fn unregister_stops_a_specific_observer() {
        let app = crate::new_app();
        app.start_runtime(50, 30);
        assert!(app.wait_barrier_for_test(Duration::from_secs(5)));

        let fired = Arc::new(AtomicBool::new(false));
        let fired_in_closure = Arc::clone(&fired);
        let id = app.register_update_frame_observer(move || {
            fired_in_closure.store(true, Ordering::SeqCst);
        });
        app.unregister_update_frame_observer(id);

        // A second, still-registered observer as a "frame reached the
        // listener thread" signal — deterministic because both observers
        // are invoked in registration order within the same
        // `notify_update_frame_observers` call, so by the time this fires
        // the unregistered one has already been skipped (or not).
        let (tx, rx) = mpsc::channel::<()>();
        app.register_update_frame_observer(move || {
            let _ = tx.send(());
        });

        app.add_relay("wss://frameobs-b.example".to_string(), "read".to_string());
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "the still-registered signal observer must fire"
        );

        assert!(
            !fired.load(Ordering::SeqCst),
            "the unregistered observer must not fire"
        );

        app.stop_runtime();
    }

    /// (c) — the single `set_update_listener` slot keeps working unchanged
    /// alongside multicast observers.
    #[test]
    fn single_update_listener_still_works_alongside_observers() {
        let app = crate::new_app();

        let (single_tx, single_rx) = mpsc::channel::<()>();
        app.set_update_listener(Some(Arc::new(move |_| {
            let _ = single_tx.send(());
        })));

        let (obs_tx, obs_rx) = mpsc::channel::<()>();
        app.register_update_frame_observer(move || {
            let _ = obs_tx.send(());
        });

        app.start_runtime(50, 30);
        assert!(app.wait_barrier_for_test(Duration::from_secs(5)));
        app.add_relay("wss://frameobs-c.example".to_string(), "read".to_string());

        assert!(
            single_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "the single update-listener slot must still fire"
        );
        assert!(
            obs_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "the multicast observer must also fire on the same frame"
        );

        app.set_update_listener(None);
        app.stop_runtime();
    }

    /// (d) — mirrors the #3078-class regression tests in
    /// `read_host_handle_tests.rs`: an observer callback that calls back
    /// into `NmpApp` (opening a nested read output) must complete rather
    /// than hang the listener thread.
    #[test]
    fn observer_callback_can_call_back_into_nmp_app_without_deadlock() {
        let app = crate::new_app();
        app.start_runtime(50, 30);
        assert!(app.wait_barrier_for_test(Duration::from_secs(5)));

        let read_host = app.read_host();
        const INNER_KEY: &str = "app.test.frame_observer_reentrant_inner";
        let opened = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<()>();
        app.register_update_frame_observer(move || {
            if !opened.swap(true, Ordering::SeqCst) {
                read_host.install_read_output(
                    DynamicProjectionKey::app_owned(INNER_KEY).unwrap().into(),
                    Box::new(|| None),
                );
            }
            let _ = tx.send(());
        });

        app.add_relay("wss://frameobs-d.example".to_string(), "read".to_string());

        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "an observer callback calling back into NmpApp (install_read_output) \
             must not deadlock the listener thread"
        );

        app.stop_runtime();
    }

    /// (e) — observers fire on the update-listener thread, never the actor
    /// thread. Compares the observer's thread id against the actor's own
    /// `JoinHandle` thread id (both accessible in-crate via `pub(crate)`
    /// fields) rather than merely against the test thread, which would not
    /// rule out a regression that ran callbacks on the actor thread.
    #[test]
    fn update_frame_observer_fires_off_the_actor_thread() {
        let app = crate::new_app();
        app.start_runtime(50, 30);
        assert!(app.wait_barrier_for_test(Duration::from_secs(5)));

        let actor_thread_id = app
            .actor
            .lock()
            .unwrap()
            .as_ref()
            .expect("actor thread must be running")
            .thread()
            .id();

        let (tx, rx) = mpsc::channel::<std::thread::ThreadId>();
        app.register_update_frame_observer(move || {
            let _ = tx.send(std::thread::current().id());
        });

        app.add_relay("wss://frameobs-e.example".to_string(), "read".to_string());

        let observer_thread_id = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("observer must fire");
        assert_ne!(
            observer_thread_id, actor_thread_id,
            "the update-frame observer must not run on the actor thread"
        );

        app.stop_runtime();
    }
}
