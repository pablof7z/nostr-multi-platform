use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use super::NmpApp;

pub(crate) type ActorStarter = Box<dyn FnOnce() -> JoinHandle<()> + Send + 'static>;

pub(crate) fn prestart_snapshot_frame(actor_queue_depth: u32) -> nmp_core::UpdateFrameBytes {
    let last_tick_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    nmp_core::encode_snapshot_frame(
        &nmp_core::SnapshotEnvelope {
            rev: 1,
            kernel_schema_version: nmp_core::SNAPSHOT_SCHEMA_VERSION,
            last_tick_ms,
            running: false,
            update_kind: "ViewBatch".to_string(),
            actor_queue_depth,
            ..Default::default()
        },
        &[],
    )
}

impl NmpApp {
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
        let depth = self
            .queue_depth
            .load(Ordering::Relaxed)
            .min(u64::from(u32::MAX)) as u32;
        let frame = prestart_snapshot_frame(depth);
        if let Ok(guard) = self.startup_update_tx.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(frame);
            }
        }
    }
}

impl Drop for NmpApp {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.update_callback.inner.lock() {
            inner.registration = None;
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
        if let Ok(mut listener) = self.update_listener.lock() {
            if let Some(handle) = listener.take() {
                let _ = handle.join();
            }
        }
    }
}
