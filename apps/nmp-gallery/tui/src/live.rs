//! Persistent live-mode kernel host for the gallery TUI.
//!
//! The gallery is **live-only** (ADR-0034 / M16): there is no fixture mode,
//! no pre-warm bootstrap, no synthesized embed envelopes. The kernel boots
//! once at program start and stays alive for the lifetime of the process.
//!
//! All data — including every embedded event in the kind-dispatch showcase —
//! flows through the standard snapshot push:
//!
//! 1. Renderer encounters an `EventRef(uri)` token.
//! 2. `NostrContentView` calls `sink.claim(uri, consumer_id)` via the
//!    `EventClaimSink` host bridge.
//! 3. `LiveKernelSink::claim` forwards to `nmp_app_claim_event` — the
//!    kernel registers a `OneshotApi` interest (D4 single writer), short-
//!    circuits on cache hit, or compiles a wire REQ on cache miss.
//! 4. The event arrives (cache or relay), gets surfaced in
//!    `snapshot.projections.claimed_events[primary_id]`, the gallery's
//!    snapshot thread sends a `GalleryEvent::Snapshot` to the main loop,
//!    `EmbedHostState::update_from_snapshot` decodes it, and the next
//!    redraw shows the resolved article (or short-note / highlight / ...).
//!
//! `LiveKernel` is `pub` so `main.rs` can keep it alive for the program
//! lifetime; `LiveKernelSink` wraps the `*mut NmpApp` pointer as the
//! `EventClaimSink` plugged into the renderer via the W4/W5 wiring.

use std::{
    ffi::{c_void, CString},
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use nmp_content::EventClaimSink;
use serde_json::Value;

/// Hex pubkey of the gallery's primary showcase author — pablof7z, the
/// NmpGallery demo account (see `nmp_core::display` tests). The user-*
/// components resolve this identity to a `ProfileWire` reactively through
/// `LiveProfileMap`; the gallery fires `claim_profile` for it at startup so
/// the kernel fetches the kind:0 and the next snapshot carries real
/// metadata.
pub const PRIMARY_PUBKEY: &str =
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";

const RELAYS: &[(&str, &str)] = &[
    ("wss://purplepag.es", "indexer"),
    // Primal serves as BOTH content AND indexer. Combined role on a
    // single RelayEditRow so `read_eligible_relay_urls` (which feeds
    // `lifecycle.set_app_relays`) picks it up. Otherwise a second
    // `add_relay` for the same URL replaces the prior role rather than
    // unioning, leaving app_relays missing.
    ("wss://relay.primal.net", "both,indexer"),
];

pub struct LiveGallerySource;

/// Persistent kernel handle. Owned by the gallery's main loop for the
/// entire process lifetime. The actor thread keeps running; snapshot pushes
/// arrive on `rx` until `Drop` tears the app down (program exit).
pub struct LiveKernel {
    /// Raw `*mut NmpApp` pointer. The actor (running on its own threads)
    /// is the single owner of the pointer's mutable state — every FFI
    /// symbol routes through its command channel. The pointer is opaque to
    /// callers and is only used to identify the app instance.
    pub app: *mut nmp_ffi::NmpApp,
    /// Keepalive for the update-callback context. Lives as long as
    /// `LiveKernel` does so the callback never sees a dangling pointer.
    bridge: Option<Box<UpdateBridge>>,
    /// Snapshot stream — taken once by `take_receiver` so the main loop
    /// can hand it to its snapshot-thread aggregator.
    rx: Option<Receiver<String>>,
}

struct UpdateBridge {
    tx: Sender<String>,
}

/// `EventClaimSink` impl wrapping a live kernel's app pointer. The
/// renderer-triggered claim path (`NostrContentView::claim_sink`) calls
/// this on each render frame; `claim` forwards to `nmp_app_claim_event`,
/// `release` to `nmp_app_release_event`. `Send + Sync` because every FFI
/// symbol forwards to the actor's command channel — the pointer is just
/// an opaque key.
pub struct LiveKernelSink {
    pub app: *mut nmp_ffi::NmpApp,
}

unsafe impl Send for LiveKernelSink {}
unsafe impl Sync for LiveKernelSink {}

impl LiveKernelSink {
    /// Trigger a kind:0 fetch for `pubkey`. Used by the gallery's main
    /// loop when a new `claimed_events` entry arrives without a cached
    /// author profile — the next snapshot tick will carry the resolved
    /// kind:0 in `mention_profiles` and the kernel's enriched
    /// `ClaimedEventDto.author_display_name` so the embed renderer can
    /// compose with `NostrProfileName` / `NostrAvatar`. Mirrors
    /// `LiveKernel::claim_profile` but available on the persistent sink
    /// the main loop holds.
    pub fn claim_profile(&self, pubkey: &str, consumer_id: &str) {
        let Ok(pk) = CString::new(pubkey) else { return };
        let Ok(cid) = CString::new(consumer_id) else { return };
        nmp_ffi::nmp_app_claim_profile(self.app, pk.as_ptr(), cid.as_ptr());
    }

    /// Open the author view for `pubkey`. Unlike a bare `claim_profile`
    /// (which only registers a kind:0 interest and caches the result), this
    /// drives the kernel's `author_view` projection — the path that surfaces
    /// the full `ProfileCard` (`nip05`, `about`, `has_profile`) and adds the
    /// author's items to `mention_profiles`. `LiveProfileMap` reads exactly
    /// that projection, so opening the primary author at startup is what
    /// lets the user-* components resolve to real kind:0 metadata instead of
    /// sitting on the npub_short fallback forever. (`mention_profiles` is
    /// built only from open-view item sets — see `kernel/update.rs` — so a
    /// standalone claim never reaches it.)
    pub fn open_author(&self, pubkey: &str) {
        let Ok(pk) = CString::new(pubkey) else { return };
        nmp_ffi::nmp_app_open_author(self.app, pk.as_ptr());
    }
}

impl EventClaimSink for LiveKernelSink {
    fn claim(&self, uri: &str, consumer_id: &str) {
        let Ok(uri_c) = CString::new(uri) else { return };
        let Ok(cid) = CString::new(consumer_id) else { return };
        nmp_ffi::nmp_app_claim_event(self.app, uri_c.as_ptr(), cid.as_ptr());
    }

    fn release(&self, uri: &str, consumer_id: &str) {
        let Ok(uri_c) = CString::new(uri) else { return };
        let Ok(cid) = CString::new(consumer_id) else { return };
        nmp_ffi::nmp_app_release_event(self.app, uri_c.as_ptr(), cid.as_ptr());
    }
}

impl LiveGallerySource {
    pub fn new(_timeout: Duration) -> Self {
        Self
    }

    /// Boot the kernel and seed the relay pool without waiting on any
    /// specific events. Used by the `--smoke` mode to validate the embed
    /// architecture in isolation from cold-start latency / relay flakes.
    /// Returns the kernel; the caller is responsible for any further
    /// data fetches via the standard claim_* / open_* FFI surface.
    pub fn boot_kernel_only() -> Result<LiveKernel, String> {
        LiveKernel::new()
    }
}

impl LiveKernel {
    pub fn new() -> Result<Self, String> {
        let app = nmp_ffi::nmp_app_new();
        if app.is_null() {
            return Err("nmp_app_new returned null".to_string());
        }
        nmp_app_gallery::nmp_app_gallery_register(app as *mut c_void);

        let (tx, rx) = std::sync::mpsc::channel();
        let mut bridge = Box::new(UpdateBridge { tx });
        let context = bridge.as_mut() as *mut UpdateBridge as *mut c_void;
        nmp_ffi::nmp_app_set_update_callback(app, context, Some(on_update));
        nmp_ffi::nmp_app_start(app, 0, 200, 8);

        let kernel = Self {
            app,
            bridge: Some(bridge),
            rx: Some(rx),
        };
        for (url, role) in RELAYS {
            kernel.add_relay(url, role)?;
        }
        Ok(kernel)
    }

    /// Take ownership of the snapshot receiver. After this call, the kernel
    /// can no longer poll its own snapshots — the main loop owns the stream.
    /// Idempotent: a second call returns `None`.
    pub fn take_receiver(&mut self) -> Option<Receiver<String>> {
        self.rx.take()
    }

    fn add_relay(&self, url: &str, role: &str) -> Result<(), String> {
        let url = CString::new(url).map_err(|_| "relay URL contains NUL byte".to_string())?;
        let role = CString::new(role).map_err(|_| "relay role contains NUL byte".to_string())?;
        nmp_ffi::nmp_app_add_relay(self.app, url.as_ptr(), role.as_ptr());
        Ok(())
    }
}

impl Drop for LiveKernel {
    fn drop(&mut self) {
        if !self.app.is_null() {
            nmp_ffi::nmp_app_set_update_callback(self.app, std::ptr::null_mut(), None);
            nmp_ffi::nmp_app_free(self.app);
            self.app = std::ptr::null_mut();
        }
        self.bridge.take();
    }
}

extern "C" fn on_update(context: *mut c_void, payload: *const u8, len: usize) {
    if context.is_null() || payload.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(payload, len) };
    let Ok(snapshot) = nmp_core::decode_snapshot_payload(bytes) else {
        return;
    };
    let bridge = unsafe { &*(context as *const UpdateBridge) };
    let _ = bridge.tx.send(snapshot.to_string());
}

/// Parse a kernel update payload (envelope or bare snapshot). Public so
/// the main loop's snapshot aggregator can decode pushed frames into the
/// `serde_json::Value` shape `EmbedHostState::update_from_snapshot` reads.
pub fn parse_snapshot(payload: &str) -> Option<Value> {
    let envelope: Value = serde_json::from_str(payload).ok()?;
    if envelope.get("t").and_then(Value::as_str) == Some("snapshot") {
        envelope.get("v").cloned()
    } else {
        Some(envelope)
    }
}
