//! Persistent live-mode kernel host for the gallery TUI.
//!
//! The gallery is **live-only** (ADR-0072 / M16): there is no fixture mode,
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
    sync::{mpsc::Receiver, Arc},
    time::Duration,
};

use nmp_content::EventRefResolver;
use nmp_core::refs::{RefEventStore, RefProfileStore, REFS_EVENT_KEY, REFS_PROFILE_KEY};
use nmp_core::typed_projections::{ClaimedEventRow, ProfileCardModel};
use nmp_native_runtime::{NmpApp, UpdateListener};

use crate::data::showcase_pubkey;

/// Hex pubkey of the gallery's primary showcase author — pablof7z.
pub fn primary_pubkey() -> &'static str {
    showcase_pubkey()
}

pub struct LiveGallerySource;

/// Decoded typed snapshot data — the gallery's view of one kernel tick.
#[derive(Debug, Default)]
pub struct GalleryTypedSnapshot {
    /// Materialised `primary_id -> ClaimedEventRow` set merged from the
    /// `refs.event` row-delta projection.
    pub events: BTreeMap<String, ClaimedEventRow>,
    /// Materialised `pubkey -> ProfileCardModel` set merged from the
    /// `refs.profile` row-delta projection (ADR-0070 #1671).
    pub profiles: BTreeMap<String, ProfileCardModel>,
    /// Per-relay connection statuses (Tier-3 envelope field).
    pub relay_statuses: Vec<nmp_core::RelayStatusEntry>,
    /// #2927 — resolved NIP-AD collections, keyed by their projection key
    /// (`nmp.nip-ad.collection.<session_id>`, the key `ad_url_state` returns in
    /// `AdUrlState::Resolved`). Decoded from the typed `ADCL` snapshot payloads.
    pub ad_collections: BTreeMap<String, nmp_nip_ad::AdCollectionSnapshot>,
}

impl GalleryTypedSnapshot {
    /// Decode a raw FlatBuffers frame, merging its row-delta batches into the
    /// persistent stores, and materialise the gallery's view of one tick.
    pub fn from_frame_bytes(
        bytes: &[u8],
        profiles_store: &mut RefProfileStore,
        events_store: &mut RefEventStore,
    ) -> Self {
        let envelope = nmp_core::decode_snapshot_envelope(bytes);
        let relay_statuses = envelope
            .as_ref()
            .map(|env| env.relay_statuses.clone())
            .unwrap_or_default();
        let session_id = envelope.as_ref().map(|env| env.session_id).unwrap_or(0);
        let snapshot_epoch = envelope.as_ref().map(|env| env.snapshot_epoch).unwrap_or(0);

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

        if let Some(payload) = find(REFS_PROFILE_KEY) {
            profiles_store.apply_sidecar(payload, session_id, snapshot_epoch);
        }
        let profiles = profiles_store.profiles();

        // #2927 — retain every resolved NIP-AD collection payload (there may be
        // several keyed by session). Decode failures are dropped (D6: a
        // malformed sidecar just means "no resolved collection yet").
        let ad_collections = typed
            .iter()
            .filter(|p| p.key.starts_with("nmp.nip-ad.collection."))
            .filter_map(|p| {
                nmp_nip_ad::decode_ad_collection_snapshot(&p.payload)
                    .ok()
                    .map(|snap| (p.key.clone(), snap))
            })
            .collect();

        Self {
            events,
            profiles,
            relay_statuses,
            ad_collections,
        }
    }

    /// True when at least one relay reports a "connected" connection state.
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
    /// Raw `*mut NmpApp` pointer owned by this handle.
    pub app: *mut NmpApp,
    /// Snapshot stream — taken once by `take_receiver`.
    rx: Option<Receiver<Vec<u8>>>,
}

/// `EventRefResolver` impl wrapping a live kernel's app pointer. The
/// renderer-triggered resolve path calls this on each render frame.
/// `Send + Sync` because every method forwards to the actor's command channel.
pub struct LiveKernelSink {
    pub app: *mut NmpApp,
}

unsafe impl Send for LiveKernelSink {}
unsafe impl Sync for LiveKernelSink {}

impl LiveKernelSink {
    /// Resolve a visible profile reference for `pubkey` (ADR-0070 #1671).
    pub fn resolve_profile(&self, pubkey: &str, consumer_id: &str) {
        self.resolve_profile_with_shape(pubkey, consumer_id, nmp_core::ProfileShape::Ref);
    }

    /// Resolve a full profile card for components that render wide fields such
    /// as NIP-05 or about text.
    pub fn resolve_profile_card(&self, pubkey: &str, consumer_id: &str) {
        self.resolve_profile_with_shape(pubkey, consumer_id, nmp_core::ProfileShape::Card);
    }

    fn resolve_profile_with_shape(
        &self,
        pubkey: &str,
        consumer_id: &str,
        shape: nmp_core::ProfileShape,
    ) {
        if self.app.is_null() {
            return;
        }
        unsafe { &*self.app }.resolve_ref(
            nmp_core::RefNamespace::Profile,
            pubkey.to_owned(),
            consumer_id.to_owned(),
            nmp_core::RefShape::Profile(shape),
            nmp_core::RefLiveness::CacheOk,
        );
    }

    /// #2927 — claim a NIP-AD candidate URL for a note authored by `author`
    /// (moment-1 render). Idempotent; policy-gated inside the runtime. The
    /// resolved collection later surfaces in the typed `ADCL` snapshot and is
    /// discoverable via [`Self::ad_url_state`].
    pub fn claim_ad_url(&self, url: &str, author: &str, consumer_id: &str) {
        if self.app.is_null() {
            return;
        }
        use nmp_content::AdUrlResolver;
        unsafe { &*self.app }.claim_ad_url(url, author, consumer_id);
    }

    /// #2927 — the current [`AdUrlState`](nmp_content::AdUrlState) for `url`
    /// (the render-side read-door: plain link vs. resolved collection).
    #[must_use]
    pub fn ad_url_state(&self, url: &str) -> nmp_content::AdUrlState {
        if self.app.is_null() {
            return nmp_content::AdUrlState::NotAttempted;
        }
        unsafe { &*self.app }.ad_url_state(url)
    }

    /// Release a profile reference previously resolved via [`Self::resolve_profile`].
    pub fn release_ref(&self, pubkey: &str, consumer_id: &str) {
        if self.app.is_null() {
            return;
        }
        unsafe { &*self.app }.release_ref(
            nmp_core::RefNamespace::Profile,
            pubkey.to_owned(),
            consumer_id.to_owned(),
        );
    }
}

struct EventRefFromUri {
    key: String,
    metadata: nmp_core::RefResolveMetadata,
}

/// Decode a `nostr:` URI or bare NIP-19 entity into a raw event key plus
/// resolver metadata. D6: non-event URIs or decode failures return `None`.
fn event_ref_from_uri(uri: &str) -> Option<EventRefFromUri> {
    let (key, relays, author) = if uri.starts_with("nostr:") {
        match nmp_nostr_id::parse_nostr_uri(uri).ok()? {
            nmp_nostr_id::NostrUri::Event {
                event_id,
                relays,
                author,
                ..
            } => (event_id, relays, author),
            nmp_nostr_id::NostrUri::Address {
                identifier,
                pubkey,
                kind,
                relays,
            } => (format!("{kind}:{pubkey}:{identifier}"), relays, None),
            _ => return None,
        }
    } else {
        match nmp_nostr_id::parse(uri).ok()? {
            nmp_nostr_id::Nip19Entity::Note(event_id) => (event_id, vec![], None),
            nmp_nostr_id::Nip19Entity::Nevent(d) => (d.event_id, d.relays, d.author),
            nmp_nostr_id::Nip19Entity::Naddr(d) => (
                format!("{}:{}:{}", d.kind, d.pubkey, d.identifier),
                d.relays,
                None,
            ),
            _ => return None,
        }
    };
    let metadata = nmp_core::RefResolveMetadata {
        hints: relays,
        event_author: author,
    };
    Some(EventRefFromUri { key, metadata })
}

impl EventRefResolver for LiveKernelSink {
    fn resolve_event_ref(&self, uri: &str, consumer_id: &str) {
        if self.app.is_null() {
            return;
        }
        let Some(event_ref) = event_ref_from_uri(uri) else {
            return;
        };
        unsafe { &*self.app }.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Event,
            event_ref.key,
            consumer_id.to_owned(),
            nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
            nmp_core::RefLiveness::CacheOk,
            event_ref.metadata,
        );
    }

    fn release_event_ref(&self, uri: &str, consumer_id: &str) {
        if self.app.is_null() {
            return;
        }
        let Some(event_ref) = event_ref_from_uri(uri) else {
            return;
        };
        unsafe { &*self.app }.release_ref(
            nmp_core::RefNamespace::Event,
            event_ref.key,
            consumer_id.to_owned(),
        );
    }
}

impl LiveGallerySource {
    pub fn new(_timeout: Duration) -> Self {
        Self
    }

    /// Boot the kernel and seed the relay pool.
    pub fn boot_kernel_only() -> Result<LiveKernel, String> {
        LiveKernel::new()
    }
}

impl LiveKernel {
    pub fn new() -> Result<Self, String> {
        let mut app = nmp_native_runtime::new_app();
        if !nmp_app_gallery::register_gallery_composition(&mut app) {
            return Err("gallery composition root already claimed".to_string());
        }
        let app = Box::into_raw(Box::new(app));

        // #2927 — inject a RESOLVING NIP-AD policy at the gallery composition
        // root so moment-1 (passive render) actually fires. The framework
        // default is `NeverAutoResolve` (no passive fetch); the gallery is a
        // demo/proof surface, so it opts into `Always` to demonstrate an AD URL
        // upgrading to its resolved collection. Real apps choose their own
        // posture (FollowsOnly / WebOfTrust / NeverAutoResolve).
        // SAFETY: app is a valid, non-null pointer (just allocated above).
        unsafe { &*app }.set_ad_resolution_policy(Arc::new(nmp_nip_ad::Always));

        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        // The tx is moved into the listener closure; the closure (inside the Arc)
        // keeps tx alive for as long as the listener is installed. No separate
        // bridge struct needed. D8: no polling, the listener fires on every kernel tick.
        let listener: UpdateListener = Arc::new(move |bytes: &[u8]| {
            let _ = tx.send(bytes.to_vec());
        });
        // SAFETY: app is a valid, non-null pointer (just allocated above).
        unsafe { &*app }.set_update_listener(Some(listener));
        unsafe { &*app }.start_runtime(200, 8);

        let kernel = Self { app, rx: Some(rx) };
        for relay in &nmp_app_gallery::showcase::references().relays {
            kernel.add_relay(&relay.url, &relay.role)?;
        }
        Ok(kernel)
    }

    /// Take ownership of the snapshot receiver. Idempotent: second call returns `None`.
    pub fn take_receiver(&mut self) -> Option<Receiver<Vec<u8>>> {
        self.rx.take()
    }

    fn add_relay(&self, url: &str, role: &str) -> Result<(), String> {
        if self.app.is_null() {
            return Err("app is null".to_string());
        }
        // SAFETY: app is a valid, non-null pointer for the lifetime of LiveKernel.
        unsafe { &*self.app }.add_relay(url.to_owned(), role.to_owned());
        Ok(())
    }
}

impl Drop for LiveKernel {
    fn drop(&mut self) {
        if !self.app.is_null() {
            // Clear the listener before dropping the app (quiescence contract).
            // SAFETY: app is a valid, non-null pointer — checked above.
            unsafe { &*self.app }.set_update_listener(None);
            // SAFETY: app was allocated by Box::into_raw(Box::new(...)).
            unsafe { drop(Box::from_raw(self.app)) };
            self.app = std::ptr::null_mut();
        }
    }
}
