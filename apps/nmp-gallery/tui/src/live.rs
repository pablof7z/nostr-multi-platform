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
//! lifetime; `LiveKernelSink` wraps the native-runtime app as the
//! `EventRefResolver` plugged into the renderer via the W4/W5 wiring.

use std::{
    collections::BTreeMap,
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    },
    time::Duration,
};

use nmp_content::EventRefResolver;
use nmp_core::refs::{RefEventStore, RefProfileStore, REFS_EVENT_KEY, REFS_PROFILE_KEY};
use nmp_core::typed_projections::{ClaimedEventRow, ProfileCardModel};
use nmp_native_runtime::{NmpApp, UpdateListener};

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
    /// Decode a raw FlatBuffers frame produced by the native-runtime update listener,
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
    /// Native runtime app. The actor is the single writer; shell calls enqueue
    /// typed commands through the runtime methods.
    pub app: Arc<NmpApp>,
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
    pub app: Arc<NmpApp>,
}

impl LiveKernelSink {
    /// Resolve a visible profile reference for `pubkey` (ADR-0063 #1671). The
    /// registry widgets (user-avatar / user-name) call this on render for each
    /// visible author; the resolved kind:0 flows back through the `refs.profile`
    /// row-delta projection (merged into [`RefProfileStore`]). Origin-blind:
    /// every visible author resolves at the feed-avatar shape `profile.ref` and
    /// `CacheOk` liveness (no per-row tailing sub) — the gallery renders only
    /// inline avatars/names, never an open-profile pane.
    pub fn resolve_profile(&self, pubkey: &str, consumer_id: &str) {
        self.app.resolve_ref(
            nmp_core::RefNamespace::Profile,
            pubkey.to_string(),
            consumer_id.to_string(),
            nmp_core::RefShape::Profile(nmp_core::ProfileShape::Ref),
            nmp_core::RefLiveness::CacheOk,
        );
    }

    /// Release a profile reference previously resolved via [`Self::resolve_profile`].
    /// Pass the SAME `(pubkey, consumer_id)` so the kernel reclaims the slot.
    pub fn release_ref(&self, pubkey: &str, consumer_id: &str) {
        self.app.release_ref(
            nmp_core::RefNamespace::Profile,
            pubkey.to_string(),
            consumer_id.to_string(),
        );
    }

    // V-112 (ADR-0042): `open_author` deleted — it wrapped the retired
    // `nmp_app_open_author` C-ABI symbol and had zero callers. Author-feed
    // demand belongs behind typed read sessions; user-avatar hydration uses
    // component-owned `resolve_profile` above.
}

// App-owned URI adapter over the typed event-ref resolve/release seams.
impl EventRefResolver for LiveKernelSink {
    fn resolve_event_ref(&self, uri: &str, consumer_id: &str) {
        let Some(event_ref) = nmp_app_gallery::event_ref_uri::event_ref_from_uri(uri) else {
            return;
        };
        let metadata = gallery_event_metadata(&event_ref.metadata_json);
        self.app.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Event,
            event_ref.key,
            consumer_id.to_string(),
            nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
            nmp_core::RefLiveness::CacheOk,
            metadata,
        );
    }

    fn release_event_ref(&self, uri: &str, consumer_id: &str) {
        let Some(event_ref) = nmp_app_gallery::event_ref_uri::event_ref_from_uri(uri) else {
            return;
        };
        self.app.release_ref(
            nmp_core::RefNamespace::Event,
            event_ref.key,
            consumer_id.to_string(),
        );
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
        let mut runtime = nmp_native_runtime::new_app();
        nmp_app_gallery::register_gallery_runtime(&mut runtime);

        let (tx, rx) = std::sync::mpsc::channel();
        let mut bridge = Box::new(UpdateBridge { tx });
        let bridge_ptr = bridge.as_mut() as *mut UpdateBridge as usize;
        runtime.set_update_listener(Some(update_listener(bridge_ptr)));
        runtime.start_runtime(200, 8);
        let app = Arc::new(runtime);

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
        self.app.add_relay(url.to_string(), role.to_string());
        Ok(())
    }
}

impl Drop for LiveKernel {
    fn drop(&mut self) {
        self.app.set_update_listener(None);
        self.app.shutdown();
        self.bridge.take();
    }
}

/// Raw kernel update callback. Sends frame bytes verbatim on the channel —
/// zero decoding here (the decode happens where the data is consumed, in the
/// snapshot thread / smoke loop). PR-B: the gallery reads only the typed
/// Tier-3 envelope + typed sidecars; `payload:Value` no longer exists.
fn on_update(context: usize, bytes: &[u8]) {
    if context == 0 {
        return;
    }
    let bridge = unsafe { &*(context as *const UpdateBridge) };
    let _ = bridge.tx.send(bytes.to_vec());
}

fn update_listener(bridge_ptr: usize) -> UpdateListener {
    Arc::new(move |bytes: &[u8]| {
        on_update(bridge_ptr, bytes);
    })
}

fn gallery_event_metadata(json: &str) -> nmp_core::RefResolveMetadata {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return nmp_core::RefResolveMetadata::default();
    };
    let hints = value
        .get("hints")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let event_author = value
        .get("author")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    nmp_core::RefResolveMetadata {
        hints,
        event_author,
    }
}
