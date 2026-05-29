//! Live kernel bridge — wraps `LiveKernel` (nmp_ffi path + relay connections)
//! and exposes a push-driven snapshot channel to the iced subscription.
//!
//! Uses the same kernel flows as `nmp-gallery-tui`: `LiveKernel::new()` boots
//! the actor, registers `nmp_app_gallery` defaults via `register_defaults`,
//! adds the gallery relays, and installs the JSON push callback. The reader
//! thread parses inbound JSON snapshots via `parse_snapshot` and sends them
//! on a tokio mpsc channel.
//!
//! Doctrine: D8 — no polling. The reader thread blocks on the kernel's
//! snapshot channel, and the iced subscription receives directly from the
//! mpsc channel (no timer, no slot polling).

use std::thread;

use nmp_gallery_tui::live::{LiveKernel, LiveKernelSink, parse_snapshot};
use serde_json::Value;
use tokio::sync::mpsc;

pub struct GalleryBridge {
    pub sink: LiveKernelSink,
    // Keep the kernel alive — its Drop frees the NmpApp the sink points into.
    _kernel: LiveKernel,
    /// Receiver for push-driven snapshots. Owned by bridge; the reader thread
    /// sends on the corresponding sender. Unbounded channel keeps the latest
    /// snapshot flowing; receiver is taken by the iced subscription.
    snapshot_rx: Option<mpsc::UnboundedReceiver<Value>>,
}

impl GalleryBridge {
    /// Boot the live kernel, register gallery defaults, seed relays, and
    /// start the reader thread with a push-driven snapshot channel.
    /// Panics on kernel boot failure (gallery is a dev tool; a failed boot
    /// is a hard error).
    pub fn start() -> Self {
        let mut kernel = LiveKernel::new().expect("LiveKernel boot failed");
        let app = kernel.app;
        let sink = LiveKernelSink { app };

        let (snapshot_tx, snapshot_rx) = mpsc::unbounded_channel();
        let rx = kernel
            .take_receiver()
            .expect("snapshot receiver available immediately after LiveKernel::new");

        thread::spawn(move || {
            for payload in rx {
                let Some(v) = parse_snapshot(&payload) else {
                    continue;
                };
                // Send on the tokio channel. Ignore send error (subscription
                // dropped); the loop exits gracefully.
                let _ = snapshot_tx.send(v);
            }
        });

        Self {
            sink,
            _kernel: kernel,
            snapshot_rx: Some(snapshot_rx),
        }
    }

    /// Forward an event claim (nevent / note / naddr URI) for embed resolution.
    pub fn claim_event(&self, uri: &str, consumer_id: &str) {
        use nmp_content::EventClaimSink;
        self.sink.claim(uri, consumer_id);
    }

    /// Forward a kind:0 profile claim into the kernel's `OneshotApi` interest
    /// registry. Idempotent per `(pubkey, consumer_id)` pair. Call on every
    /// poll tick so the claim sticks once a relay connects (the kernel
    /// silently drops claims issued before any relay is ready).
    pub fn claim_profile(&self, pubkey: &str, consumer_id: &str) {
        self.sink.claim_profile(pubkey, consumer_id);
    }

    /// Take the snapshot receiver for use in the iced subscription. Called
    /// once at startup; subsequent calls return None.
    pub fn take_snapshot_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<Value>> {
        self.snapshot_rx.take()
    }
}
