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
//! 2. `NostrContentView` calls `sink.resolve_event_ref(uri, consumer_id)` via the
//!    `EventRefResolver` host bridge.
//! 3. `LiveKernelSink::resolve_event_ref` forwards the raw event key plus
//!    decoded URI metadata to the typed event-ref adapter — the
//!    kernel registers a `OneshotApi` interest (D4 single writer), short-circuits
//!    on cache hit, or compiles a wire REQ on cache miss.
//! 4. The event arrives (cache or relay), gets surfaced in the typed
//!    `refs.event` row-delta sidecar, the gallery's snapshot thread sends a
//!    `GalleryEvent::Snapshot` to the main loop, `EmbedHostState::update_from_typed`
//!    consumes the materialised event rows, and the next
//!    redraw shows the resolved article (or short-note / highlight / ...).
//!
//! `LiveKernel` is `pub` so `main.rs` can keep it alive for the program
//! lifetime; `LiveKernelSink` wraps the `*mut NmpApp` pointer as the
//! `EventRefResolver` plugged into the renderer via the W4/W5 wiring.

use std::{
    collections::BTreeMap,
    ffi::{c_void, CString},
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use nmp_content::EventRefResolver;
use nmp_core::refs::{RefEventStore, RefProfileStore, REFS_EVENT_KEY, REFS_PROFILE_KEY};
use nmp_core::typed_projections::{ClaimedEventRow, ProfileCardModel};

use crate::data::showcase_pubkey;

/// Hex pubkey of the gallery's primary showcase author — pablof7z, the
/// NmpGallery showcase identity. The user-*
/// components resolve this identity to a `ProfileWire` reactively through
/// `LiveProfileMap`; `tui/user-avatar` fires `resolve_profile` when rendered so
/// the kernel fetches the kind:0 and a later snapshot carries real metadata.
pub fn primary_pubkey() -> &'static str {
    showcase_pubkey()
}

pub struct LiveGallerySource;

/// Decoded typed snapshot data — the gallery's view of one kernel tick.
///
/// Built from the FlatBuffers typed-sidecar payload that the kernel emits on
/// every actor tick (ADR-0037). The gallery reads:
/// - `events` — the materialised `primary_id -> ClaimedEventRow` set merged
///   from the `refs.event` row-delta projection (embed host, EmbedHostState).
/// - `profiles` — the materialised `pubkey -> ProfileCardModel` set merged from
///   the `refs.profile` row-delta projection (ADR-0063 #1671). This replaces the
///   retired `resolved_profiles` whole-map projection. Because `refs.profile` is
///   a per-KEY row-delta batch, it can only be merged into the stateful
///   [`RefProfileStore`] that lives across frames. `refs.event` follows the same
///   row-grained contract through [`RefEventStore`] — see
///   [`GalleryTypedSnapshot::from_frame_bytes`].
///
/// Both fields degrade gracefully to empty when their respective typed sidecar
/// entry is absent or fails to decode (D6 — no panic, no blank-render reset).
///
/// The `relay_statuses` field carries the Tier-3 relay-connection vector (from
/// the typed `SnapshotEnvelope`), used by the smoke-mode relay-wait loop.
#[derive(Debug, Default)]
pub struct GalleryTypedSnapshot {
    /// Materialised `primary_id -> ClaimedEventRow` set merged from the
    /// `refs.event` row-delta projection. Empty when no event refs are live or
    /// after the kernel has explicitly cleared every row.
    pub events: BTreeMap<String, ClaimedEventRow>,
    /// Materialised `pubkey -> ProfileCardModel` set merged from the
    /// `refs.profile` row-delta projection (resolve_ref output). Empty until the
    /// kernel has resolved at least one kind:0 for a resolved pubkey. This is a
    /// per-frame snapshot of the stateful [`RefProfileStore`]; it is NOT a second
    /// app-side cache (D4) — the store is the sole owner.
    pub profiles: BTreeMap<String, ProfileCardModel>,
    /// Per-relay connection statuses (Tier-3 envelope field). Used by the
    /// smoke mode to detect when at least one relay is connected.
    pub relay_statuses: Vec<nmp_core::RelayStatusEntry>,
}

impl GalleryTypedSnapshot {
    /// Decode a raw FlatBuffers frame (as produced by `nmp_app_set_update_callback`),
    /// merging its `refs.profile` / `refs.event` row-delta batches into the
    /// persistent stores, and materialise the gallery's view of one tick.
    ///
    /// ADR-0063 (#1671): both refs projections are per-KEY row-deltas, so the
    /// caller MUST thread stateful stores (one per update loop) — a single
    /// frame's batch carries only changed/cleared rows. Tolerant: if any
    /// projection fails to decode its field is left at the default (empty). A
    /// malformed refs payload is a fail-closed no-op inside its store (prior rows
    /// retained). Panics never occur (D6).
    pub fn from_frame_bytes(
        bytes: &[u8],
        profiles_store: &mut RefProfileStore,
        events_store: &mut RefEventStore,
    ) -> Self {
        // Tier-3 envelope: relay_statuses + the (session_id, snapshot_epoch)
        // identity the row-delta cache merges under.
        let envelope = nmp_core::decode_snapshot_envelope(bytes);
        let relay_statuses = envelope
            .as_ref()
            .map(|env| env.relay_statuses.clone())
            .unwrap_or_default();
        let session_id = envelope.as_ref().map(|env| env.session_id).unwrap_or(0);
        let snapshot_epoch = envelope.as_ref().map(|env| env.snapshot_epoch).unwrap_or(0);

        // Typed sidecar entries.
        let typed = nmp_core::decode_snapshot_typed_projections(bytes).unwrap_or_default();

        let find = |key: &str| -> Option<&[u8]> {
            typed
                .iter()
                .find(|p| p.key == key)
                .map(|p| p.payload.as_slice())
        };

        if let Some(payload) = find(REFS_EVENT_KEY) {
            events_store.apply_sidecar(payload, session_id, snapshot_epoch);
        }
        let events = events_store.events();

        // ADR-0063 (#1671): merge the `refs.profile` row-delta batch into the
        // stateful store (the ONLY app-side mirror of hydrated profiles, D4),
        // then materialise the current full set for this frame's readers.
        if let Some(payload) = find(REFS_PROFILE_KEY) {
            profiles_store.apply_sidecar(payload, session_id, snapshot_epoch);
        }
        let profiles = profiles_store.profiles();

        Self {
            events,
            profiles,
            relay_statuses,
        }
    }

    /// True when at least one relay reports a "connected" connection state.
    /// Used by the smoke loop to gate the first event-ref resolve.
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

/// `EventRefResolver` impl wrapping a live kernel's app pointer. The
/// renderer-triggered resolve path (`NostrContentView::event_ref_resolver`) calls
/// this on each render frame; `resolve_event_ref` forwards to the typed event-embed adapter,
/// `release_event_ref` to the typed event-ref release adapter. `Send + Sync` because every FFI
/// symbol forwards to the actor's command channel — the pointer is just
/// an opaque key.
pub struct LiveKernelSink {
    pub app: *mut nmp_ffi::NmpApp,
}

unsafe impl Send for LiveKernelSink {}
unsafe impl Sync for LiveKernelSink {}

impl LiveKernelSink {
    /// Resolve a visible profile reference for `pubkey` (ADR-0063 #1671). The
    /// registry widgets (user-avatar / user-name) call this on render for each
    /// visible author; the resolved kind:0 flows back through the `refs.profile`
    /// row-delta projection (merged into [`RefProfileStore`]). Origin-blind:
    /// every visible author resolves at the feed-avatar shape `profile.ref` and
    /// `CacheOk` liveness (no per-row tailing sub) — the gallery renders only
    /// inline avatars/names, never an open-profile pane.
    pub fn resolve_profile(&self, pubkey: &str, consumer_id: &str) {
        let Ok(pk) = CString::new(pubkey) else { return };
        let Ok(cid) = CString::new(consumer_id) else {
            return;
        };
        nmp_ffi::nmp_app_resolve_profile_ref(self.app, pk.as_ptr(), cid.as_ptr());
    }

    /// Release a profile reference previously resolved via [`Self::resolve_profile`].
    /// Pass the SAME `(pubkey, consumer_id)` so the kernel reclaims the slot.
    pub fn release_ref(&self, pubkey: &str, consumer_id: &str) {
        let Ok(pk) = CString::new(pubkey) else { return };
        let Ok(cid) = CString::new(consumer_id) else {
            return;
        };
        nmp_ffi::nmp_app_release_profile_ref(self.app, pk.as_ptr(), cid.as_ptr());
    }

    // V-112 (ADR-0042): `open_author` deleted — it wrapped the retired
    // `nmp_app_open_author` C-ABI symbol and had zero callers. Author feeds
    // go through the generic `nmp_app_open_interest` seam; user-avatar
    // hydration uses component-owned `resolve_profile` above.
}

struct EventRefFromUri {
    key: CString,
    metadata_json: CString,
}

/// Decode a `nostr:` URI via `nmp_nip21_decode_uri` and return the canonical
/// raw event key plus metadata the kernel resolver expects:
///   - nevent / note  → hex event_id
///   - naddr          → canonical coordinate "kind:pubkey:identifier"
/// Returns `None` on decode failure or non-event/address target (D6: silent
/// no-op). Used by `LiveKernelSink` before it calls the raw-key ref seam.
fn event_ref_from_uri(uri: &str) -> Option<EventRefFromUri> {
    let uri_c = CString::new(uri).ok()?;
    let raw = nmp_ffi::nmp_nip21_decode_uri(uri_c.as_ptr());
    if raw.is_null() {
        return None;
    }
    let s = unsafe { std::ffi::CStr::from_ptr(raw) }
        .to_str()
        .ok()
        .map(str::to_owned);
    nmp_ffi::nmp_free_string(raw);
    let s = s?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    if !v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false) {
        return None;
    }
    let key = match v.get("target").and_then(|t| t.as_str()) {
        Some("event") => v.get("event_id").and_then(|e| e.as_str())?.to_owned(),
        Some("address") => {
            let kind = v.get("kind").and_then(|k| k.as_u64())?;
            let pubkey = v.get("pubkey").and_then(|p| p.as_str())?;
            let identifier = v.get("identifier").and_then(|i| i.as_str())?;
            format!("{kind}:{pubkey}:{identifier}")
        }
        _ => return None,
    };
    let relays: Vec<String> = v
        .get("relays")
        .and_then(|r| r.as_array())?
        .iter()
        .map(|relay| relay.as_str().map(str::to_owned))
        .collect::<Option<_>>()?;
    let mut metadata = serde_json::json!({ "hints": relays });
    if let Some(author) = v.get("author").and_then(|a| a.as_str()) {
        metadata["author"] = serde_json::Value::String(author.to_string());
    }
    if let Some(kind) = v.get("kind").and_then(|k| k.as_u64()) {
        metadata["kind"] = serde_json::Value::Number(kind.into());
    }
    Some(EventRefFromUri {
        key: CString::new(key).ok()?,
        metadata_json: CString::new(metadata.to_string()).ok()?,
    })
}

// App-owned URI adapter over the typed event-ref resolve/release seams.
impl EventRefResolver for LiveKernelSink {
    fn resolve_event_ref(&self, uri: &str, consumer_id: &str) {
        let Some(event_ref) = event_ref_from_uri(uri) else {
            return;
        };
        let Ok(cid) = CString::new(consumer_id) else {
            return;
        };
        nmp_ffi::nmp_app_resolve_event_embed_with_metadata(
            self.app,
            event_ref.key.as_ptr(),
            cid.as_ptr(),
            event_ref.metadata_json.as_ptr(),
        );
    }

    fn release_event_ref(&self, uri: &str, consumer_id: &str) {
        let Some(event_ref) = event_ref_from_uri(uri) else {
            return;
        };
        let Ok(cid) = CString::new(consumer_id) else {
            return;
        };
        nmp_ffi::nmp_app_release_event_ref(self.app, event_ref.key.as_ptr(), cid.as_ptr());
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
