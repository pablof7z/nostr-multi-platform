use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::thread::JoinHandle;

use super::NmpApp;
use crate::UpdateListener;

pub(crate) type ActorStarter = Box<dyn FnOnce() -> JoinHandle<()> + Send + 'static>;

pub(crate) fn prestart_snapshot_frame(
    queue_depth: &Arc<AtomicU64>,
    clock_slot: &nmp_core::slots::KernelClockSlot,
) -> nmp_core::UpdateFrameBytes {
    let clock = clock_slot.lock().ok().and_then(|guard| guard.clone());
    nmp_core::KernelReducer::passive_snapshot_frame(clock, Arc::clone(queue_depth))
}

impl NmpApp {
    pub fn set_update_listener(&self, listener: Option<UpdateListener>) {
        let Ok(guard) = self.update_listener.inner.lock() else {
            return;
        };
        let mut guard = guard;
        guard.listener = listener;
        let waited = self
            .update_listener
            .drained
            .wait_while(guard, |inner| inner.in_flight > 0);
        drop(waited);
        if self
            .update_listener
            .inner
            .lock()
            .map(|inner| inner.listener.is_some())
            .unwrap_or(false)
            && !self.started.load(std::sync::atomic::Ordering::SeqCst)
        {
            self.emit_passive_prestart_snapshot();
        }
    }

    pub(crate) fn spawn_actor_if_needed(&self) {
        let Some(starter) = self.actor_starter.lock().ok().and_then(|mut g| g.take()) else {
            return;
        };
        if let Ok(mut startup_tx) = self.startup_update_tx.lock() {
            startup_tx.take();
        }
        let handle = starter();
        if let Ok(mut actor) = self.actor.lock() {
            *actor = Some(handle);
        }
    }

    pub(crate) fn emit_passive_prestart_snapshot(&self) {
        let frame = prestart_snapshot_frame(&self.queue_depth, &self.composition.kernel_clock);
        if let Ok(guard) = self.startup_update_tx.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(frame);
            }
        }
    }
}

impl Drop for NmpApp {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.update_listener.inner.lock() {
            inner.listener = None;
        }
        self.capability_callback.clear();
        // Route through `shutdown_actor` (→ `send_cmd`) so the G-S4 queue-depth
        // counter stays consistent: the actor decrements it as it dequeues `Shutdown`.
        self.shutdown_actor();
        if let Ok(mut starter) = self.actor_starter.lock() {
            starter.take();
        }
        if let Ok(mut startup_tx) = self.startup_update_tx.lock() {
            startup_tx.take();
        }
        if let Ok(mut actor) = self.actor.lock() {
            if let Some(handle) = actor.take() {
                let _ = handle.join();
            }
        }
        if let Ok(mut listener) = self.update_listener_thread.lock() {
            if let Some(handle) = listener.take() {
                let _ = handle.join();
            }
        }
    }
}
