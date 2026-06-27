//! Kernel update-callback state for push delivery.
//!
//! Factored out of `session.rs` to keep that file under the 500-LOC ceiling.
//! (AGENTS.md §File-Size)

use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;

/// Holds the delivery channels for kernel update frames.
///
/// Fields are `pub(crate)` so the sibling test submodule (`tests.rs`)
/// can observe slot state directly.
pub(crate) struct CallbackState {
    /// Legacy mpsc sink — retained only for the in-crate unit tests
    /// (`recv_next_update`). Production update delivery is the UniFFI generic
    /// sink via [`CallbackState::set_generic_sink`].
    pub(crate) tx: Mutex<Option<Sender<Vec<u8>>>>,
    /// UniFFI generic sink — invoked on every update frame for the UniFFI
    /// app-loop lane (M14-0 / issue #2129, D8: no polling). Snapshot-under-lock
    /// pattern: `Arc` is cloned under the `Mutex` lock, the lock is released
    /// BEFORE the invocation so the lock is never held across the Kotlin upcall
    /// (deadlock prevention). Cleared in `close_updates` after the quiescence
    /// gate (same as the former push_listener).
    pub(crate) generic_sink: Mutex<Option<Arc<dyn Fn(Vec<u8>) + Send + Sync>>>,
}

impl CallbackState {
    pub(crate) fn new(tx: Sender<Vec<u8>>) -> Self {
        Self {
            tx: Mutex::new(Some(tx)),
            generic_sink: Mutex::new(None),
        }
    }

    pub(crate) fn send(&self, bytes: Vec<u8>) {
        let Ok(guard) = self.tx.lock() else {
            return;
        };
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(bytes);
        }
    }

    pub(crate) fn close(&self) {
        if let Ok(mut guard) = self.tx.lock() {
            guard.take();
        }
    }

    /// Register the UniFFI generic update sink (M14-0 / issue #2129).
    /// The Arc wrapper lets `on_update` snapshot it under a brief lock and
    /// release the lock before invocation (deadlock prevention).
    pub(crate) fn set_generic_sink(&self, sink: Arc<dyn Fn(Vec<u8>) + Send + Sync>) {
        if let Ok(mut slot) = self.generic_sink.lock() {
            *slot = Some(sink);
        }
    }

    /// Clear the UniFFI generic sink slot.  Safe to call when none is set.
    /// Must only be called AFTER the quiescence gate
    /// (`nmp_app_set_update_callback(None)`) returns — that guarantees no
    /// in-flight `on_update` can touch the slot after this clears it.
    pub(crate) fn clear_generic_sink(&self) {
        if let Ok(mut slot) = self.generic_sink.lock() {
            slot.take();
        }
    }
}
