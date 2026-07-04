//! `open_ad_collection` — the NIP-AD collection delivery doorway (#2948 / #2927 B-C).
//!
//! This is the delivery half of NIP-AD. It takes a resolved
//! [`AdResolution`](crate::AdResolution) `{ filter, relays }` and drives it
//! through the shared read-session engine as a set of ONE-SHOT, relay-pinned
//! collection reads — one per site-supplied relay. It rides
//! `InterestLifecycle::OneShot` (threaded through the read-demand path in #2948):
//! the collection completes and tears down on EOSE. If a site later wants a
//! live-updating collection, flip the single flag to `Tailing`; no
//! re-architecture.
//!
//! A NIP-AD collection is a live query that may return 0..N events
//! (`golf.com/highlights` → many kind:20; `trellis.rs/legible` → one kind:30023).
//! There is NO `limit` and NO reduction to a single pointer — the full deduped
//! set is delivered.
//!
//! Layering (read-door doctrine, #2777): this concept crate depends on
//! `nmp-read-session` (the engine), NEVER on a runtime crate. Only the
//! concept-named `open_ad_collection` doorway crosses outward; the generic
//! read-lifecycle machinery stays inside the engine. Per-row *rendering*
//! dispatches by `kind` through the existing embed pipeline
//! (`nmp_content::resolve_embed_projection`) downstream (#2927 B/C) — this
//! doorway only produces the typed, deduped, ordered snapshot.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use nmp_ownership::FrameworkProjectionKey;
use nmp_read_session::{
    close_read, open_read, InterestLifecycle, ReadDemand, ReadHandle, ReadHost, ReadOutputEncoder,
    ReadReplayPolicy, ReadSpec,
};
use serde::{Deserialize, Serialize};

use crate::wire::{encode_ad_collection_snapshot, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION};
use crate::AdResolution;

/// `1` = Global. AD collections pin concrete site-supplied relays and are not
/// re-routed on account switch (identical to NIP-50 search); callers
/// close/reopen to change identity.
const AD_COLLECTION_SCOPE_GLOBAL: u32 = 1;

/// Newest-first window cap for the per-relay cache-replay seed. The live REQ is
/// still un-floored (a OneShot backfill); this only bounds the structural
/// replay that primes the observer before activation.
const AD_COLLECTION_REPLAY_LIMIT: usize = 256;

/// One deduplicated collection row (one delivered event), carrying raw protocol
/// values only. No display logic — per-kind rendering is downstream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdCollectionRow {
    /// Raw hex event id (dedup key).
    pub id: String,
    /// Raw hex author pubkey.
    pub author: String,
    /// Event kind (the per-row render dispatch discriminant).
    pub kind: u32,
    /// Raw signed `created_at`, Unix seconds.
    pub created_at: u64,
    /// Raw event content.
    pub content: String,
    /// Raw protocol tags.
    pub tags: Vec<Vec<String>>,
    /// Relays that delivered this event id.
    pub relay_provenance: Vec<String>,
}

impl AdCollectionRow {
    /// Reconstruct the raw [`KernelEvent`] this row was ingested from, so a
    /// host can drive it back through the per-kind render pipeline
    /// (`nmp_content::resolve_embed_projection`) exactly like any other embedded
    /// event (#2927 B/C render bridge). The row carries only raw protocol values
    /// (no signature — rendering never needs one), so the mapping is 1:1.
    #[must_use]
    pub fn to_kernel_event(&self) -> KernelEvent {
        KernelEvent {
            id: self.id.clone(),
            author: self.author.clone(),
            kind: self.kind,
            created_at: self.created_at,
            tags: self.tags.clone(),
            content: self.content.clone(),
            relay_provenance: self.relay_provenance.clone(),
        }
    }
}

/// The typed collection snapshot: the full deduped set, newest-first.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdCollectionSnapshot {
    /// Deduplicated rows, ordered `created_at` desc then id-stable.
    pub rows: Vec<AdCollectionRow>,
}

/// The AD collection read model: dedupe-by-id (first arrival wins), ordered
/// newest-first on snapshot.
#[derive(Default)]
pub struct AdCollectionProjection {
    rows: BTreeMap<String, AdCollectionRow>,
}

impl AdCollectionProjection {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingest one delivered event. First arrival wins on a duplicate id; the
    /// delivering relay is unioned into the row's provenance.
    pub fn ingest_relay_event(&mut self, event: &KernelEvent, relay_url: String) {
        if self.rows.contains_key(&event.id) {
            return;
        }
        let mut relay_provenance = event.relay_provenance.clone();
        if !relay_url.is_empty() && !relay_provenance.contains(&relay_url) {
            relay_provenance.push(relay_url);
        }
        self.rows.insert(
            event.id.clone(),
            AdCollectionRow {
                id: event.id.clone(),
                author: event.author.clone(),
                kind: event.kind,
                created_at: event.created_at,
                content: event.content.clone(),
                tags: event.tags.clone(),
                relay_provenance,
            },
        );
    }

    /// The deduped rows ordered `created_at` desc, id-stable ascending tiebreak
    /// (matches the NIP-50 projection's deterministic order).
    #[must_use]
    pub fn snapshot(&self) -> AdCollectionSnapshot {
        let mut rows: Vec<AdCollectionRow> = self.rows.values().cloned().collect();
        rows.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| a.id.cmp(&b.id)));
        AdCollectionSnapshot { rows }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// The snapshot-projection key for one AD collection session.
#[must_use]
pub fn ad_collection_projection_key(session_id: &str) -> String {
    format!("nmp.nip-ad.collection.{session_id}")
}

/// Refcount-owner key for one relay's collection demand within a session.
#[must_use]
pub fn ad_collection_consumer(session_id: &str, relay: &str) -> String {
    format!("ad-collection-{session_id}-{relay}")
}

/// Build the per-relay one-shot demands for a resolution. Factored out of
/// [`open_ad_collection`] so the demand shape is unit-testable without a host.
///
/// For EACH relay in `resolution.relays`, one `ReadDemand` pinned to that relay
/// carrying the resolution's full filter serialized to NIP-01 JSON,
/// `lifecycle: OneShot`, `scope: Global`, structural replay. N relays = N pinned
/// one-shot demands, all folding into the session's single reducer (mirrors
/// NIP-50 search's per-relay pinned demands).
#[must_use]
pub fn ad_collection_demands(resolution: &AdResolution, session_id: &str) -> Vec<ReadDemand> {
    // `nostr::Filter` serializes to a canonical NIP-01 filter object; never a
    // hand-rolled encode. This is the same filter JSON the read engine parses
    // into an InterestShape.
    let filter_json =
        serde_json::to_string(&resolution.filter).unwrap_or_else(|_| "{}".to_string());
    resolution
        .relays
        .iter()
        .map(|relay| ReadDemand {
            filter_json: filter_json.clone(),
            consumer_id: ad_collection_consumer(session_id, relay),
            scope: AD_COLLECTION_SCOPE_GLOBAL,
            relay_pin: Some(relay.clone()),
            is_indexer_discovery: false,
            // The load-bearing bit unblocked by #2948: a collection query that
            // completes on EOSE. Flip to `Tailing` for a live-updating door.
            lifecycle: InterestLifecycle::OneShot,
            replay_limit: AD_COLLECTION_REPLAY_LIMIT,
            replay: ReadReplayPolicy::Structural,
        })
        .collect()
}

struct AdCollectionObserver(Arc<Mutex<AdCollectionProjection>>);

impl ObservedProjectionSink for AdCollectionObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let relay = event.relay_provenance.first().cloned().unwrap_or_default();
        if let Ok(mut projection) = self.0.lock() {
            projection.ingest_relay_event(event, relay);
        }
    }
}

/// Runtime close/read handle for one NIP-AD collection read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdCollectionReadHandle(ReadHandle);

impl AdCollectionReadHandle {
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Open a NIP-AD collection read through the shared read-session engine.
///
/// Drives one relay-pinned OneShot [`ReadDemand`] per resolved relay into the
/// single dedupe/order reducer, installs the typed `ADCL` snapshot under the
/// per-session key, and returns the engine-owned close handle. An empty
/// `resolution.relays` is fail-open: the (empty) typed output is still
/// installed and a live handle is returned, so the caller sees an empty
/// collection rather than an error.
#[must_use]
pub fn open_ad_collection(
    host: &dyn ReadHost,
    resolution: &AdResolution,
    session_id: &str,
) -> AdCollectionReadHandle {
    let _ = close_ad_collection_by_key(host, session_id);

    let key = ad_collection_projection_key(session_id);
    let projection = Arc::new(Mutex::new(AdCollectionProjection::new()));
    let demands = ad_collection_demands(resolution, session_id);

    let projection_for_output = Arc::clone(&projection);
    let output_key = key.clone();
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.lock().ok()?.snapshot();
        Some(TypedProjectionData {
            key: output_key.clone(),
            schema_id: SCHEMA_ID.to_string(),
            schema_version: SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(FILE_IDENTIFIER).into_owned(),
            payload: encode_ad_collection_snapshot(&snapshot),
            ..Default::default()
        })
    });

    let projection_key = FrameworkProjectionKey::declared(key, "projection.nmp.nip-ad.collection")
        .expect("AD collection keys use the nmp.nip-ad.collection family");
    let handle = open_read(
        host,
        ReadSpec {
            projection_key: projection_key.into(),
            demands,
            observer: Arc::new(AdCollectionObserver(projection)) as Arc<dyn ObservedProjectionSink>,
            output_encoder,
            dependent_demands: Vec::new(),
            // Fail-open: a resolution with no relays still installs an empty
            // typed output and returns a live handle.
            keep_open_without_live_demand: true,
        },
    );

    AdCollectionReadHandle(handle)
}

/// Close an AD collection read by its engine-owned handle.
#[must_use]
pub fn close_ad_collection(host: &dyn ReadHost, handle: &AdCollectionReadHandle) -> bool {
    close_read(host, &handle.0)
}

/// Close an AD collection read by its stable session key.
#[must_use]
pub fn close_ad_collection_by_key(host: &dyn ReadHost, session_id: &str) -> bool {
    host.close_read_session_by_projection_key(&ad_collection_projection_key(session_id))
}

#[cfg(test)]
#[path = "collection_tests.rs"]
mod tests;
