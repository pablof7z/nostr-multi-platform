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
//! 4. The event arrives (cache or relay), gets surfaced in the typed
//!    `claimed_events` sidecar, the gallery's snapshot thread sends a
//!    `GalleryEvent::Snapshot` to the main loop,
//!    `EmbedHostState::update_from_typed` decodes it, and the next
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
use nmp_core::typed_projections::{
    ClaimedEventsModel, CLAIMED_EVENTS_SCHEMA_ID,
    ResolvedProfilesModel, RESOLVED_PROFILES_SCHEMA_ID,
};

use crate::data::showcase_pubkey;

/// Hex pubkey of the gallery's primary showcase author — pablof7z, the
/// NmpGallery showcase identity. The user-*
/// components resolve this identity to a `ProfileWire` reactively through
/// `LiveProfileMap`; `tui/user-avatar` fires `claim_profile` when rendered so
/// the kernel fetches the kind:0 and a later snapshot carries real metadata.
pub fn primary_pubkey() -> &'static str {
    showcase_pubkey()
}

pub struct LiveGallerySource;

/// Decoded typed snapshot data — the gallery's view of one kernel tick.
///
/// Built from the FlatBuffers typed-sidecar payload that the kernel emits on
/// every actor tick (ADR-0037). The gallery reads two projection keys:
/// - `claimed_events` — resolved embed events (embed host, EmbedHostState).
/// - `resolved_profiles` — pre-merged pubkey→ProfileCard map (LiveProfileMap).
///
/// Both fields degrade gracefully to empty when their respective typed sidecar
/// entry is absent or fails to decode (D6 — no panic, no blank-render reset).
///
/// The `relay_statuses` field carries the Tier-3 relay-connection vector (from
/// the typed `SnapshotEnvelope`), used by the smoke-mode relay-wait loop.
#[derive(Debug, Default)]
pub struct GalleryTypedSnapshot {
    /// Resolved embed events. Empty when no claims have been issued yet or
    /// while relay round-trips are in flight.
    pub claimed_events: ClaimedEventsModel,
    /// Pre-merged pubkey→ProfileCard map. Empty until the kernel has ingested
    /// at least one kind:0 for a claimed pubkey.
    pub resolved_profiles: ResolvedProfilesModel,
    /// Per-relay connection statuses (Tier-3 envelope field). Used by the
    /// smoke mode to detect when at least one relay is connected.
    pub relay_statuses: Vec<nmp_core::RelayStatusEntry>,
}

impl GalleryTypedSnapshot {
    /// Decode a raw FlatBuffers frame (as produced by `nmp_app_set_update_callback`)
    /// into a `GalleryTypedSnapshot`. Tolerant: if any projection fails to decode
    /// its field is left at the default (empty). Panics never occur (D6).
    pub fn from_frame_bytes(bytes: &[u8]) -> Self {
        // Tier-3 envelope: relay_statuses.
        let relay_statuses = nmp_core::decode_snapshot_envelope(bytes)
            .map(|env| env.relay_statuses)
            .unwrap_or_default();

        // Typed sidecar entries.
        let typed = nmp_core::decode_snapshot_typed_projections(bytes).unwrap_or_default();

        let find = |key: &str| -> Option<&[u8]> {
            typed
                .iter()
                .find(|p| p.key == key)
                .map(|p| p.payload.as_slice())
        };

        let claimed_events = find(CLAIMED_EVENTS_SCHEMA_ID)
            .and_then(|b| nmp_core::typed_projections::decode_claimed_events(b).ok())
            .unwrap_or_default();

        let resolved_profiles = find(RESOLVED_PROFILES_SCHEMA_ID)
            .and_then(|b| nmp_core::typed_projections::decode_resolved_profiles(b).ok())
            .unwrap_or_default();

        Self {
            claimed_events,
            resolved_profiles,
            relay_statuses,
        }
    }

    /// True when at least one relay reports a "connected" connection state.
    /// Used by the smoke loop to gate the first claim issuance.
    pub fn any_relay_connected(&self) -> bool {
        self.relay_statuses
            .iter()
            .any(|r| r.connection == "connected")
    }
}

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
    rx: Option<Receiver<Vec<u8>>>,
}

struct UpdateBridge {
    tx: Sender<Vec<u8>>,
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
    /// Trigger a kind:0 fetch for `pubkey`. Registry widgets use this for
    /// visible profile references; the next snapshot carries the resolved
    /// profile through `claimed_profiles`.
    pub fn claim_profile(&self, pubkey: &str, consumer_id: &str) {
        let Ok(pk) = CString::new(pubkey) else { return };
        let Ok(cid) = CString::new(consumer_id) else {
            return;
        };
        // F-TTL — component-owned profile self-claim on render → force = 0.
        nmp_ffi::nmp_app_claim_profile(self.app, pk.as_ptr(), cid.as_ptr(), 0, 0);
    }

    pub fn release_profile(&self, pubkey: &str, consumer_id: &str) {
        let Ok(pk) = CString::new(pubkey) else { return };
        let Ok(cid) = CString::new(consumer_id) else {
            return;
        };
        nmp_ffi::nmp_app_release_profile(self.app, pk.as_ptr(), cid.as_ptr());
    }

    // V-112 (ADR-0042): `open_author` deleted — it wrapped the retired
    // `nmp_app_open_author` C-ABI symbol and had zero callers. Author feeds
    // go through the generic `nmp_app_open_interest` seam; user-avatar
    // hydration uses component-owned `claim_profile` above.
}

impl EventClaimSink for LiveKernelSink {
    fn claim(&self, uri: &str, consumer_id: &str) {
        let Ok(uri_c) = CString::new(uri) else { return };
        let Ok(cid) = CString::new(consumer_id) else {
            return;
        };
        // F-TTL — embed sink claims on render → force = 0 (background path).
        nmp_ffi::nmp_app_claim_event(self.app, uri_c.as_ptr(), cid.as_ptr(), 0);
    }

    fn release(&self, uri: &str, consumer_id: &str) {
        let Ok(uri_c) = CString::new(uri) else { return };
        let Ok(cid) = CString::new(consumer_id) else {
            return;
        };
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
        nmp_ffi::nmp_app_start(app, 200, 8);

        let kernel = Self {
            app,
            bridge: Some(bridge),
            rx: Some(rx),
        };
        for relay in &nmp_app_gallery::showcase::references().relays {
            kernel.add_relay(&relay.url, &relay.role)?;
        }
        Ok(kernel)
    }

    /// Take ownership of the snapshot receiver. After this call, the kernel
    /// can no longer poll its own snapshots — the main loop owns the stream.
    /// Idempotent: a second call returns `None`.
    pub fn take_receiver(&mut self) -> Option<Receiver<Vec<u8>>> {
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

/// Raw kernel update callback. Sends frame bytes verbatim on the channel —
/// zero decoding here (the decode happens where the data is consumed, in the
/// snapshot thread / smoke loop). PR-B: the gallery reads only the typed
/// Tier-3 envelope + typed sidecars; `payload:Value` no longer exists.
extern "C" fn on_update(context: *mut c_void, payload: *const u8, len: usize) {
    if context.is_null() || payload.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(payload, len) };
    let bridge = unsafe { &*(context as *const UpdateBridge) };
    let _ = bridge.tx.send(bytes.to_vec());
}
