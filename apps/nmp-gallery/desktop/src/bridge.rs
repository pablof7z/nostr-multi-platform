//! Live kernel bridge — wraps `LiveKernel` (nmp_ffi path + relay connections)
//! and exposes a snapshot slot the iced poll subscription drains each tick.
//!
//! Uses the same kernel flows as `nmp-gallery-tui`: `LiveKernel::new()` boots
//! the actor, registers `nmp_app_gallery` defaults via `register_defaults`,
//! adds the gallery relays, and installs the JSON push callback. The reader
//! thread parses inbound JSON snapshots via `parse_snapshot` and stores the
//! latest in `Arc<Mutex<Option<Value>>>`.
//!
//! Doctrine: D8 — no polling in the reader thread; it blocks on
//! `Receiver<String>::recv` (the snapshot channel is push-driven by the
//! kernel's emit tick). The iced subscription polls the slot at ~4 Hz to
//! match the kernel's own emit_hz.

use std::sync::{Arc, Mutex};
use std::thread;

use nmp_gallery_tui::live::{LiveKernel, LiveKernelSink, parse_snapshot};
use serde_json::Value;

pub struct GalleryBridge {
    pub sink: LiveKernelSink,
    // Keep the kernel alive — its Drop frees the NmpApp the sink points into.
    _kernel: LiveKernel,
    latest: Arc<Mutex<Option<Value>>>,
}

impl GalleryBridge {
    /// Boot the live kernel, register gallery defaults, seed relays, and
    /// start the reader thread. Panics on kernel boot failure (gallery is a
    /// dev tool; a failed boot is a hard error).
    pub fn start() -> Self {
        let mut kernel = LiveKernel::new().expect("LiveKernel boot failed");
        let app = kernel.app;
        let sink = LiveKernelSink { app };

        let latest: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
        let rx = kernel
            .take_receiver()
            .expect("snapshot receiver available immediately after LiveKernel::new");

        let writer = Arc::clone(&latest);
        thread::spawn(move || {
            for payload in rx {
                let Some(v) = parse_snapshot(&payload) else {
                    continue;
                };
                if let Ok(mut slot) = writer.lock() {
                    *slot = Some(v);
                }
            }
        });

        Self {
            sink,
            _kernel: kernel,
            latest,
        }
    }

    /// Take the latest snapshot (clears the slot — the iced update loop only
    /// processes each snapshot once, keeping `update_from_snapshot` from
    /// rerunning against the same data every poll tick).
    pub fn take_snapshot(&self) -> Option<Value> {
        self.latest.lock().ok().and_then(|mut s| s.take())
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
}
