//! Lifecycle UniFFI methods — C6 tail.
//!
//! Mirrors `nmp-ffi/src/lifecycle.rs` without the C function-pointer trampoline.
//! The callback setter uses `LifecycleObserverGate`'s Rust-native observer path,
//! so C and UniFFI lifecycle observers share the same `in_flight` + `Condvar`
//! drain contract.

use std::sync::Arc;

use nmp_core::__ffi_internal::NativeLifecycleObserver;

use crate::{LifecycleSink, NmpApp};

#[uniffi::export]
impl NmpApp {
    /// Report that the host app entered the foreground. Fire-and-forget; the
    /// actor folds the phase into Rust-owned lifecycle state.
    pub fn lifecycle_foreground(&self) {
        self.inner.lifecycle_foreground();
    }

    /// Report that the host app entered the background. Fire-and-forget; the
    /// actor folds the phase into Rust-owned lifecycle state.
    pub fn lifecycle_background(&self) {
        self.inner.lifecycle_background();
    }

    /// Register or clear the lifecycle callback.
    ///
    /// After this returns, the previous sink is neither registered nor
    /// mid-invocation. Pass `None` to clear.
    pub fn set_lifecycle_callback(&self, sink: Option<Box<dyn LifecycleSink>>) {
        let observer: Option<NativeLifecycleObserver> = sink.map(|s| {
            let s: Arc<dyn LifecycleSink> = Arc::from(s);
            Arc::new(move |phase: u32| {
                let s = Arc::clone(&s);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    s.on_lifecycle_phase(phase);
                }));
            }) as NativeLifecycleObserver
        });
        self.inner.set_native_lifecycle_observer(observer);
    }

    /// Return whether the actor thread is alive.
    pub fn is_alive(&self) -> bool {
        self.inner.is_alive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LifecycleSink;
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier, Mutex};
    use std::time::Duration;

    struct RecordLifecycleSink {
        phases: Arc<Mutex<Vec<u32>>>,
        tx: Mutex<Option<mpsc::Sender<()>>>,
    }

    impl LifecycleSink for RecordLifecycleSink {
        fn on_lifecycle_phase(&self, phase: u32) {
            self.phases.lock().unwrap().push(phase);
            if let Ok(mut guard) = self.tx.lock() {
                let _ = guard.take().map(|tx| tx.send(()));
            }
        }
    }

    struct BlockingLifecycleSink {
        entered_tx: Mutex<Option<mpsc::Sender<()>>>,
        gate: Arc<Barrier>,
    }

    impl LifecycleSink for BlockingLifecycleSink {
        fn on_lifecycle_phase(&self, _phase: u32) {
            if let Ok(mut guard) = self.entered_tx.lock() {
                let _ = guard.take().map(|tx| tx.send(()));
            }
            self.gate.wait();
        }
    }

    #[test]
    fn lifecycle_callback_fires_after_foreground() {
        let app = NmpApp::new();
        let phases = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel::<()>();
        app.set_lifecycle_callback(Some(Box::new(RecordLifecycleSink {
            phases: Arc::clone(&phases),
            tx: Mutex::new(Some(tx)),
        })));
        app.start(256, 4);
        app.lifecycle_foreground();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("lifecycle callback fired");

        assert_eq!(
            phases.lock().unwrap().as_slice(),
            &[nmp_core::__ffi_internal::LIFECYCLE_PHASE_FOREGROUND],
        );
        app.set_lifecycle_callback(None);
        app.shutdown();
    }

    #[test]
    fn lifecycle_callback_clear_waits_for_in_flight() {
        let app = NmpApp::new();
        let gate = Arc::new(Barrier::new(2));
        let (entered_tx, entered_rx) = mpsc::channel::<()>();

        app.set_lifecycle_callback(Some(Box::new(BlockingLifecycleSink {
            entered_tx: Mutex::new(Some(entered_tx)),
            gate: Arc::clone(&gate),
        })));
        app.start(256, 4);
        app.lifecycle_foreground();
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("lifecycle callback entered");

        let app_for_clear = Arc::clone(&app);
        let (clear_started_tx, clear_started_rx) = mpsc::channel::<()>();
        let (clear_done_tx, clear_done_rx) = mpsc::channel::<()>();
        let clear = std::thread::spawn(move || {
            clear_started_tx.send(()).unwrap();
            app_for_clear.set_lifecycle_callback(None);
            clear_done_tx.send(()).unwrap();
        });
        clear_started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("clear started");
        assert!(
            clear_done_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "clear returned while lifecycle callback was in-flight",
        );

        gate.wait();
        clear.join().unwrap();
        clear_done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("clear returns after lifecycle callback drains");
        app.shutdown();
    }
}
