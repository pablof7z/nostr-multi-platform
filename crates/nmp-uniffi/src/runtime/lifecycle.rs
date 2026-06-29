//! App-lifecycle UniFFI methods — M14-C6.
//!
//! Migrates three of the four C-ABI symbols from `nmp-ffi/src/lifecycle.rs`
//! to typed `#[uniffi::export] impl NmpApp` methods:
//!
//! | UniFFI method          | C-ABI counterpart                  |
//! |------------------------|------------------------------------|
//! | `lifecycle_foreground` | `nmp_app_lifecycle_foreground`     |
//! | `lifecycle_background` | `nmp_app_lifecycle_background`     |
//! | `is_alive`             | `nmp_app_is_alive`                 |
//!
//! `nmp_app_set_lifecycle_callback` is now mirrored by `set_lifecycle_sink`
//! (M14-C-tail / #2429), routed through the `nmp-core`
//! `LifecycleObserverGate`'s `in_flight` + `Condvar` drain. The C-ABI symbol
//! stays additive until M14-D.
//!
//! ## Doctrine
//!
//! * `lifecycle_foreground` / `lifecycle_background` / `is_alive` are
//!   fire-and-forget or pull-only; they carry no callback and need no
//!   quiescence contract.
//! * `set_lifecycle_sink` registers a `LifecycleSink` ARC behind the drain
//!   gate: after it returns, the previous sink is neither registered nor
//!   mid-invocation, so the host may release it.
//! * D6: `lifecycle_foreground` / `lifecycle_background` dispatch commands
//!   best-effort on the actor channel (a closed channel is a silent no-op).
//!   `is_alive` is a lock-based probe that never panics. A panicking
//!   `LifecycleSink` is caught inside the wrapper (never unwinds into the
//!   actor thread).

use std::sync::Arc;

use crate::runtime::LifecycleSink;
use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Register (or clear) the lifecycle-transition observer.
    ///
    /// The actor calls `on_lifecycle_transition(phase)` on a meaningful
    /// scenePhase change (foreground/background), where `phase` is the wire
    /// discriminant (`0` = foreground, `1` = background).
    ///
    /// Pass `None` to clear. After this returns, the previous sink is
    /// guaranteed to be neither registered nor mid-invocation (the
    /// `LifecycleObserverGate` `in_flight` + Condvar drain), so the host may
    /// release it immediately. Shares one gate with the C-ABI
    /// `nmp_app_set_lifecycle_callback` (last-writer-wins).
    ///
    /// Re-entrancy is forbidden: calling this from inside
    /// `on_lifecycle_transition` deadlocks the quiescence gate.
    pub fn set_lifecycle_sink(&self, sink: Option<Box<dyn LifecycleSink>>) {
        let observer: Option<nmp_core::__ffi_internal::NativeLifecycleObserver> = sink.map(|s| {
            // Wrap in Arc so the closure is `Sync` (NativeLifecycleObserver
            // requires `Send + Sync`).
            let s: Arc<dyn LifecycleSink> = Arc::from(s);
            Arc::new(move |phase: u32| {
                // `phase` is a copied `u32` — no Rust lock is held here. Panic
                // containment: a Swift/Kotlin throw must not unwind into the
                // actor thread (D6).
                let s = Arc::clone(&s);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    s.on_lifecycle_transition(phase);
                }));
            }) as nmp_core::__ffi_internal::NativeLifecycleObserver
        });
        self.inner.set_lifecycle_native_observer(observer);
    }
    /// Report the platform entering the foreground (`scenePhase == .active` on
    /// iOS, or equivalent). Fire-and-forget.
    ///
    /// The actor folds the phase into the kernel and fires the registered
    /// lifecycle observer on a `Background → Foreground` (or first-after-boot)
    /// transition. Repeated `Foreground` calls debounce to a no-op.
    ///
    /// D6: a dead actor (channel closed) silently drops the command.
    pub fn lifecycle_foreground(&self) {
        self.inner.lifecycle_foreground();
    }

    /// Report the platform entering the background (`scenePhase == .background`
    /// on iOS, or equivalent). Fire-and-forget. Symmetric to
    /// [`lifecycle_foreground`].
    ///
    /// D6: a dead actor silently drops the command.
    pub fn lifecycle_background(&self) {
        self.inner.lifecycle_background();
    }

    /// Actor-liveness probe: returns `true` when the actor `JoinHandle` is
    /// still running, `false` otherwise.
    ///
    /// This is the pull-side companion to the `UpdateEnvelope::Panic` push
    /// frame (D7): a host that missed the panic frame while backgrounded can
    /// call this on resume to learn the same fact.
    ///
    /// Returns `false` before `start()` or after the actor has exited (clean
    /// shutdown or panic).
    pub fn is_alive(&self) -> bool {
        self.inner.is_alive()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::runtime::LifecycleSink;
    use crate::NmpApp;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    /// Records every phase code it receives and signals each delivery on a
    /// channel so tests can wait deterministically (no sleeps) for the
    /// actor-thread callback to land.
    struct SignalSink {
        phases: Arc<Mutex<Vec<u32>>>,
        fired_tx: mpsc::SyncSender<u32>,
    }

    impl LifecycleSink for SignalSink {
        fn on_lifecycle_transition(&self, phase: u32) {
            self.phases.lock().unwrap().push(phase);
            let _ = self.fired_tx.send(phase);
        }
    }

    /// Signals entry via `started_tx`, blocks on `release_rx` (main-thread
    /// controlled), then increments `completed` AFTER release.
    struct BlockingSink {
        started_tx: Mutex<Option<mpsc::SyncSender<()>>>,
        release_rx: Mutex<mpsc::Receiver<()>>,
        completed: Arc<AtomicU32>,
    }

    impl LifecycleSink for BlockingSink {
        fn on_lifecycle_transition(&self, _phase: u32) {
            if let Ok(mut g) = self.started_tx.lock() {
                let _ = g.take().map(|tx| tx.send(()));
            }
            let _ = self.release_rx.lock().unwrap().recv();
            self.completed.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Register → fire → clear → replace: sink A receives the foreground
    /// transition; after clear A stops firing and a freshly registered sink B
    /// receives the next (background) transition. Also exercises idempotent
    /// clear.
    #[test]
    fn lifecycle_sink_register_fire_clear_replace() {
        let app = NmpApp::new();
        let phases_a = Arc::new(Mutex::new(Vec::new()));
        let (fired_a_tx, fired_a_rx) = mpsc::sync_channel(1);
        app.set_lifecycle_sink(Some(Box::new(SignalSink {
            phases: Arc::clone(&phases_a),
            fired_tx: fired_a_tx,
        })));
        app.start(256, 4);
        app.lifecycle_foreground();

        // Deterministically wait for A's foreground delivery (no sleep).
        let p = fired_a_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("sink A should receive the foreground transition");
        assert_eq!(p, super::super::LIFECYCLE_PHASE_FOREGROUND);

        // Clear A (drains), then idempotent second clear.
        app.set_lifecycle_sink(None);
        app.set_lifecycle_sink(None);

        // Register B and fire a background transition.
        let phases_b = Arc::new(Mutex::new(Vec::new()));
        let (fired_b_tx, fired_b_rx) = mpsc::sync_channel(1);
        app.set_lifecycle_sink(Some(Box::new(SignalSink {
            phases: Arc::clone(&phases_b),
            fired_tx: fired_b_tx,
        })));
        app.lifecycle_background();

        let p = fired_b_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("sink B should receive the background transition");
        assert_eq!(p, super::super::LIFECYCLE_PHASE_BACKGROUND);

        // A saw only foreground; B saw only background — clear/replace routed
        // correctly.
        assert_eq!(
            phases_a.lock().unwrap().as_slice(),
            &[super::super::LIFECYCLE_PHASE_FOREGROUND],
            "cleared sink A must not have received the background transition"
        );
        assert_eq!(
            phases_b.lock().unwrap().as_slice(),
            &[super::super::LIFECYCLE_PHASE_BACKGROUND],
        );
        app.shutdown();
    }

    /// THE drain proof: `set_lifecycle_sink(None)` must block while the sink is
    /// mid-flight on the actor thread and return only after it completes.
    #[test]
    fn lifecycle_sink_clear_waits_for_in_flight() {
        let app = NmpApp::new();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let completed = Arc::new(AtomicU32::new(0));
        app.set_lifecycle_sink(Some(Box::new(BlockingSink {
            started_tx: Mutex::new(Some(started_tx)),
            release_rx: Mutex::new(release_rx),
            completed: Arc::clone(&completed),
        })));
        app.start(256, 4);
        // Fire the transition; the actor thread enters the sink and blocks.
        app.lifecycle_foreground();
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("sink should enter on the actor thread");

        let (clear_done_tx, clear_done_rx) = mpsc::sync_channel(1);
        let app_for_clear = Arc::clone(&app);
        let clear = thread::spawn(move || {
            app_for_clear.set_lifecycle_sink(None);
            clear_done_tx.send(()).unwrap();
        });

        // Negative check: the setter must NOT return while the sink is mid-flight.
        assert!(
            clear_done_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "set_lifecycle_sink(None) returned while the sink was mid-flight — quiescence violated"
        );
        release_tx.send(()).unwrap();
        clear_done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("set_lifecycle_sink(None) should return after the sink drains");

        clear.join().unwrap();
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        app.shutdown();
    }

    /// `shutdown()` while a lifecycle sink is mid-flight on the actor thread must
    /// not UAF or deadlock: `NmpApp::drop` clears the lifecycle gate (draining
    /// the in-flight call) before joining the actor.
    #[test]
    fn lifecycle_sink_shutdown_during_in_flight_no_uaf() {
        let app = NmpApp::new();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let completed = Arc::new(AtomicU32::new(0));
        app.set_lifecycle_sink(Some(Box::new(BlockingSink {
            started_tx: Mutex::new(Some(started_tx)),
            release_rx: Mutex::new(release_rx),
            completed: Arc::clone(&completed),
        })));
        app.start(256, 4);
        app.lifecycle_foreground();
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("sink should enter on the actor thread");

        // Shutdown in its own thread (it will block draining the sink), then
        // release so it can complete — bounded by a wall-clock deadline.
        let app_for_shutdown = Arc::clone(&app);
        let (sd_tx, sd_rx) = mpsc::sync_channel(1);
        let shutdown = thread::spawn(move || {
            app_for_shutdown.shutdown();
            sd_tx.send(()).unwrap();
        });
        release_tx.send(()).unwrap();
        sd_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("shutdown() deadlocked while the lifecycle sink was in-flight");

        shutdown.join().unwrap();
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        app.shutdown(); // idempotent
    }

    /// Parity with `nmp_app_is_alive` C-ABI test
    /// `is_alive_after_new_returns_zero_before_start`: `is_alive()` must
    /// return `false` before `start()`.
    #[test]
    fn parity_is_alive_false_before_start() {
        let app = NmpApp::new();
        assert!(
            !app.is_alive(),
            "actor must not be alive before start()"
        );
    }

    /// Parity with `nmp_app_is_alive` C-ABI test
    /// `is_alive_after_new_returns_zero_before_start` (post-start part):
    /// `is_alive()` returns `true` after `start()`.
    #[test]
    fn parity_is_alive_true_after_start() {
        let app = NmpApp::new();
        app.start(256, 4);
        assert!(app.is_alive(), "actor must be alive after start()");
        app.shutdown();
    }

    /// Parity with the C-ABI foreground/background tests:
    /// `lifecycle_foreground` and `lifecycle_background` must not panic and
    /// must be callable before and after `start()`.
    #[test]
    fn parity_lifecycle_signals_no_panic() {
        let app = NmpApp::new();
        // Before start: commands queue (passive handle).
        app.lifecycle_foreground();
        app.lifecycle_background();
        app.start(256, 4);
        // After start: commands reach the actor.
        app.lifecycle_foreground();
        app.lifecycle_background();
        app.shutdown();
    }

    /// `lifecycle_foreground` after shutdown is a silent no-op (D6:
    /// closed channel drops the send).
    #[test]
    fn parity_lifecycle_after_shutdown_no_panic() {
        let app = NmpApp::new();
        app.start(256, 4);
        app.shutdown();
        // Must not panic; the actor channel is closed.
        app.lifecycle_foreground();
        app.lifecycle_background();
    }
}
