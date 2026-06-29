//! Live kernel bridge — wraps `LiveKernel` (nmp_ffi path + relay connections)
//! and exposes a push-driven typed-snapshot channel to the iced subscription.
//!
//! Uses the same kernel flows as `nmp-gallery-tui`: `LiveKernel::new()` boots
//! the actor, registers the gallery compatibility composition, adds the gallery
//! relays, and installs the raw-bytes push callback. The reader
//! thread decodes inbound FlatBuffers frames via `GalleryTypedSnapshot::from_frame_bytes`
//! and sends them on a tokio mpsc channel.
//!
//! Doctrine: D8 — no polling. The reader thread blocks on the kernel's
//! snapshot channel, and the iced subscription receives directly from the
//! mpsc channel (no timer, no slot polling).

use std::thread;

use nmp_gallery_tui::live::{GalleryTypedSnapshot, LiveKernel, LiveKernelSink};
use tokio::sync::mpsc;

pub struct GalleryBridge {
    pub sink: LiveKernelSink,
    // Keep the kernel alive — its Drop frees the NmpApp the sink points into.
    _kernel: LiveKernel,
    /// Receiver for push-driven typed snapshots. Owned by bridge; the reader thread
    /// sends on the corresponding sender. Unbounded channel keeps the latest
    /// snapshot flowing; receiver is taken by the iced subscription.
    snapshot_rx: Option<mpsc::UnboundedReceiver<GalleryTypedSnapshot>>,
}

impl GalleryBridge {
    /// Boot the live kernel, register gallery defaults, seed relays, and
    /// start the reader thread with a push-driven typed-snapshot channel.
    /// Panics on kernel boot failure (gallery is a dev tool; a failed boot
    /// is a hard error).
    pub fn start() -> Self {
        let mut kernel = LiveKernel::new().expect("LiveKernel boot failed");
        let app = kernel.app.clone();
        let sink = LiveKernelSink { app };

        let (snapshot_tx, snapshot_rx) = mpsc::unbounded_channel();
        let rx = kernel
            .take_receiver()
            .expect("snapshot receiver available immediately after LiveKernel::new");

        thread::spawn(move || {
            // ADR-0063 (#1671): the stateful `refs.profile` row-delta mirror lives
            // in the reader thread, merged across frames so per-key deltas
            // accumulate. It is the sole app-side profile store (D4); each frame
            // materialises its current set into the snapshot.
            let mut ref_profiles = nmp_core::refs::RefProfileStore::new();
            let mut ref_events = nmp_core::refs::RefEventStore::new();
            for frame_bytes in rx {
                let snap = GalleryTypedSnapshot::from_frame_bytes(
                    &frame_bytes,
                    &mut ref_profiles,
                    &mut ref_events,
                );
                // Send on the tokio channel. Ignore send error (subscription
                // dropped); the loop exits gracefully.
                let _ = snapshot_tx.send(snap);
            }
        });

        Self {
            sink,
            _kernel: kernel,
            snapshot_rx: Some(snapshot_rx),
        }
    }

    /// Resolve an event URI (nevent / note / naddr) for embed rendering.
    pub fn resolve_event_uri(&self, uri: &str, consumer_id: &str) {
        use nmp_content::EventRefResolver;
        self.sink.resolve_event_ref(uri, consumer_id);
    }

    /// Resolve a visible profile reference (ADR-0063 #1671 — `resolve_ref` at
    /// `profile.ref` / `CacheOk`). Idempotent per `(pubkey, consumer_id)` pair.
    /// Call on every poll tick so the resolution sticks once a relay connects
    /// (the kernel silently drops requests issued before any relay is ready).
    pub fn resolve_profile(&self, pubkey: &str, consumer_id: &str) {
        self.sink.resolve_profile(pubkey, consumer_id);
    }

    /// Take the snapshot receiver for use in the iced subscription. Called
    /// once at startup; subsequent calls return None.
    pub fn take_snapshot_receiver(
        &mut self,
    ) -> Option<mpsc::UnboundedReceiver<GalleryTypedSnapshot>> {
        self.snapshot_rx.take()
    }
}
