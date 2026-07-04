//! `open_reposts` — the concept-owned repost-count active read (#2758, #2508).
//!
//! This is the repost owner's DOOR. It composes the NIP-18 repost wrapper
//! kinds (kind:6 and `k`-tag-discriminated kind:16) into demand + admission +
//! reducer + typed output, then drives them through the ONE read-lifecycle
//! engine (`nmp-read-session`). It contains NO registry, NO close map, NO
//! replay implementation, and NO teardown recipe of its own — those are the
//! engine's, reached only via [`open_read`] / [`close_read`]. If this file
//! ever grew any of them, the engine boundary would be wrong (#2777).
//!
//! The symbol lives HERE, in the concept crate: a kernel that does not import
//! `nmp-reposts` has no `open_reposts`. `open_reposts` takes the engine's
//! host seam (`&dyn ReadHost`, which a runtime like `NmpApp` implements once)
//! — it never depends on a runtime crate. Dependency direction:
//! `nmp-reposts` → `nmp-read-session` ← runtime.
//!
//! # Deletion handling
//!
//! A reposting pubkey's own kind:5 NIP-09 delete retracts their repost —
//! [`RepostSummaryProjection`] decodes deletes through `nmp_nip09::DeleteRecord`
//! (the canonical delete read seam, ADR-0074) and removes an entry only when
//! the delete's author matches the stored repost's author, exactly mirroring
//! `nmp_nip18::RepostActivityProjection::apply_delete` and
//! `nmp_nip25`'s reaction-aggregate delete fold. Because NIP-09 names the
//! *deleted event's own id* in its `e` tags (the repost wrapper's id, not the
//! target's), so kind:5 routing must grow as repost wrappers are admitted. This
//! crate declares that derived shape from its current accepted wrapper set; the
//! read-session engine owns open/replay/live replacement and teardown, so this
//! concept still has no private subscription loop.
//!
//! # Viewer-relative facts
//!
//! `count` and `reposter_pubkeys` are the raw distinct-reposter facts (D0);
//! whether the ACTIVE user is among them is derived by the shell comparing
//! its own already-available active-account pubkey against
//! `reposter_pubkeys` — this concept never depends on viewer identity, so
//! `open_reposts` takes no viewer parameter.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use flatbuffers::FlatBufferBuilder;
use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use nmp_nip09::KIND_DELETION;
use nmp_ownership::FrameworkProjectionKey;
use nmp_planner::InterestShape;
use nmp_read_session::{
    close_read, open_read, InterestLifecycle, ReadDemand, ReadDependentDemand,
    ReadDependentDemandProvider, ReadHost,
    ReadOutputEncoder, ReadReplayPolicy, ReadSpec,
};
use serde::{Deserialize, Serialize};

use crate::read::RepostReadPlan;
use crate::target::{RepostTarget, RepostTargetError};

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/repost_summary_generated.rs"]
mod generated;

use generated::nmp::reposts as fb;

/// Stable schema id for the repost-summary typed projection.
pub const REPOST_SUMMARY_SCHEMA_ID: &str = "nmp.reposts.summary";
/// Schema version mirrored in `repost_summary.fbs`.
pub const REPOST_SUMMARY_SCHEMA_VERSION: u32 = 1;
/// FlatBuffers file identifier for the repost-summary buffer.
pub const REPOST_SUMMARY_FILE_IDENTIFIER: &[u8] = b"NRPS";
/// Account-agnostic scope: a note's reposts come from anywhere, routed by the
/// target's outbox rather than the viewer's account.
const REPOST_READ_SCOPE_GLOBAL: u32 = 1;
/// Bounded read-cache replay depth before live activation.
const REPOST_REPLAY_LIMIT: usize = 512;

/// Every `open_reposts` call gets a unique output key, so independent
/// repost-count components on the same target are fully separate reads
/// (feed's unique-output model), not a shared singleton.
static NEXT_REPOST_READ: AtomicU64 = AtomicU64::new(1);

/// The repost-summary read model for ONE target: distinct reposter count +
/// their raw pubkeys. Raw data only (aim.md Sec2).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepostSummarySnapshot {
    /// The raw target event id.
    pub target_id: String,
    /// Distinct reposter count (one repost per pubkey counts once).
    pub count: u64,
    /// Raw hex pubkeys of the accepted reposters, ascending.
    pub reposter_pubkeys: Vec<String>,
}

/// The admission-applying repost reducer: the concept's event fold. It
/// ingests candidate repost-wrapper events (delivered by the demand filter)
/// and keeps only those the plan ACCEPTS as true reposts of the target,
/// keyed by the wrapper's own event id so a same-author kind:5 delete can
/// retract exactly that wrapper without discarding the author's other
/// surviving reposts of the same target. This is the ONLY concept-owned
/// stateful piece — it holds no lifecycle, only the read model.
pub struct RepostSummaryProjection {
    plan: RepostReadPlan,
    // repost wrapper event id -> reposter pubkey.
    accepted: Mutex<BoundedMessageMap<String, String>>,
}

impl RepostSummaryProjection {
    #[must_use]
    fn new(plan: RepostReadPlan) -> Self {
        Self {
            plan,
            accepted: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// The current repost summary (pubkeys ascending, count = distinct
    /// reposters).
    #[must_use]
    pub fn snapshot(&self) -> RepostSummarySnapshot {
        let target_id = self.plan.target_event_id().to_string();
        let Ok(accepted) = self.accepted.lock() else {
            return RepostSummarySnapshot {
                target_id,
                ..Default::default()
            };
        };
        let reposter_pubkeys: std::collections::BTreeSet<String> =
            accepted.values().cloned().collect();
        let reposter_pubkeys: Vec<String> = reposter_pubkeys.into_iter().collect();
        RepostSummarySnapshot {
            target_id,
            count: reposter_pubkeys.len() as u64,
            reposter_pubkeys,
        }
    }

    fn ingest_delete(&self, event: &KernelEvent) {
        // Delegate tag parsing to the nmp-nip09 read seam so tag grammar
        // interpretation is centralised in the deletion owner (ADR-0074).
        let deleted_ids = nmp_nip09::DeleteRecord::try_from_kernel_event(event)
            .map(|record| record.event_targets)
            .unwrap_or_default();
        if deleted_ids.is_empty() {
            return;
        }
        if let Ok(mut accepted) = self.accepted.lock() {
            for id in deleted_ids {
                // Only the original reposter may retract their own repost.
                if accepted
                    .get(&id)
                    .is_some_and(|author| author == &event.author)
                {
                    accepted.remove(&id);
                }
            }
        }
    }

    fn delete_demand(&self) -> Option<ReadDependentDemand> {
        let Ok(accepted) = self.accepted.lock() else {
            return None;
        };
        if accepted.is_empty() {
            return None;
        }
        let wrapper_ids = accepted
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<BTreeSet<_>>();
        let mut tags = BTreeMap::new();
        tags.insert("e".to_string(), wrapper_ids);
        Some(ReadDependentDemand {
            shape: InterestShape {
                authors: accepted.values().cloned().collect::<BTreeSet<_>>(),
                kinds: BTreeSet::from([KIND_DELETION]),
                tags,
                ..Default::default()
            },
            scope: REPOST_READ_SCOPE_GLOBAL,
            is_indexer_discovery: false,
            replay_limit: REPOST_REPLAY_LIMIT,
        })
    }
}

impl ObservedProjectionSink for RepostSummaryProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind == KIND_DELETION {
            self.ingest_delete(event);
            return;
        }
        if let Some(author) = self.plan.accepts_repost(event) {
            if let Ok(mut accepted) = self.accepted.lock() {
                accepted.insert(event.id.clone(), author);
            }
        }
    }
}

/// The typed close handle `open_reposts` returns. Wraps the engine's opaque
/// handle so a repost read can only be closed with [`close_reposts`] (not
/// with a feed or reply handle), and exposes the projection key the shell
/// renders from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepostsReadHandle(nmp_read_session::ReadHandle);

impl RepostsReadHandle {
    /// The projection key this read's typed [`RepostSummarySnapshot`] surfaces
    /// under. The shell learns it from the handle and renders that key.
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.0.projection_key
    }

    /// Decompose into the FFI-marshalable scalar parts `(projection_key,
    /// handle_id)` (#2899 Part A bridge lane). A facade returns this pair
    /// from its generated `open_reposts` door instead of inventing its own
    /// handle-map bookkeeping; [`Self::from_parts`] reconstructs the typed
    /// handle from the same pair to close.
    #[must_use]
    pub fn into_parts(self) -> (String, u64) {
        (self.0.projection_key, self.0.session_id.0)
    }

    /// Reconstruct the typed close handle from the scalar parts returned by
    /// [`Self::into_parts`]. The typed-wrapper property is preserved: this
    /// newtype — never the raw parts — is what [`close_reposts`] accepts, so
    /// a repost read still cannot be closed with a feed/reply/reaction/zap
    /// handle.
    #[must_use]
    pub fn from_parts(projection_key: String, handle_id: u64) -> Self {
        Self(nmp_read_session::ReadHandle {
            projection_key,
            session_id: nmp_read_session::ReadSessionId(handle_id),
        })
    }
}

/// Open a live repost-count read for the plain kind:1 note `target_event_id`
/// on the read-lifecycle engine.
///
/// Composes the NIP-18 repost wrapper kinds into one read: a single routed
/// demand folding into one admission-applying reducer that emits a typed
/// [`RepostSummarySnapshot`]. Returns a close handle; [`close_reposts`]
/// withdraws the demand and tombstones the output.
///
/// # Errors
///
/// Returns [`RepostTargetError`] when `target_event_id` is not a 64-hex
/// Nostr event id.
pub fn open_reposts(
    host: &dyn ReadHost,
    target_event_id: impl Into<String>,
) -> Result<RepostsReadHandle, RepostTargetError> {
    let target = RepostTarget::note(target_event_id)?;
    let plan = RepostReadPlan::new(&target);
    let token = target.event_id().to_string();
    let nonce = NEXT_REPOST_READ.fetch_add(1, Ordering::Relaxed);
    let key_string = format!("nmp.reposts.summary.{token}.{nonce}");

    let demand = ReadDemand {
        filter_json: plan.filter_json(),
        consumer_id: key_string.clone(),
        scope: REPOST_READ_SCOPE_GLOBAL,
        relay_pin: None,
        is_indexer_discovery: false,
        lifecycle: InterestLifecycle::Tailing,
        replay_limit: REPOST_REPLAY_LIMIT,
        replay: ReadReplayPolicy::Structural,
    };

    let projection = Arc::new(RepostSummaryProjection::new(plan));
    let dependent_demands: Vec<ReadDependentDemandProvider> = {
        let projection = Arc::clone(&projection);
        vec![Arc::new(move || projection.delete_demand())]
    };

    // Typed output: encode the reducer's snapshot each tick. Coalesced
    // emission + tombstone-on-close are the engine/runtime's, not this
    // closure's.
    let projection_for_output = Arc::clone(&projection);
    let output_key = key_string.clone();
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.snapshot();
        Some(TypedProjectionData {
            key: output_key.clone(),
            schema_id: REPOST_SUMMARY_SCHEMA_ID.to_string(),
            schema_version: REPOST_SUMMARY_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(REPOST_SUMMARY_FILE_IDENTIFIER).into_owned(),
            payload: encode_repost_summary_snapshot(&snapshot),
            ..Default::default()
        })
    });

    // `nmp.reposts.*` is a framework prefix, so this declaration cannot fail.
    // The owner-claim literal must appear here for the crate-ownership audit.
    let projection_key =
        FrameworkProjectionKey::declared(key_string, "projection.nmp.reposts.summary")
            .expect("nmp.reposts.summary.* carries the framework prefix");

    let handle = open_read(
        host,
        ReadSpec {
            projection_key: projection_key.into(),
            demands: vec![demand],
            observer: projection as Arc<dyn ObservedProjectionSink>,
            output_encoder,
            dependent_demands,
            keep_open_without_live_demand: false,
        },
    );
    Ok(RepostsReadHandle(handle))
}

/// Close a repost read opened by [`open_reposts`], withdrawing the demand and
/// tombstoning the typed output. Idempotent (D6).
pub fn close_reposts(host: &dyn ReadHost, handle: RepostsReadHandle) -> bool {
    close_read(host, &handle.0)
}

/// Encode a [`RepostSummarySnapshot`] to its typed FlatBuffers payload.
#[must_use]
pub fn encode_repost_summary_snapshot(snapshot: &RepostSummarySnapshot) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let target_id = fbb.create_string(&snapshot.target_id);
    let pubkey_offsets: Vec<_> = snapshot
        .reposter_pubkeys
        .iter()
        .map(|pubkey| fbb.create_string(pubkey))
        .collect();
    let reposter_pubkeys = fbb.create_vector(&pubkey_offsets);
    let root = fb::RepostSummarySnapshot::create(
        &mut fbb,
        &fb::RepostSummarySnapshotArgs {
            schema_version: REPOST_SUMMARY_SCHEMA_VERSION,
            target_id: Some(target_id),
            count: snapshot.count,
            reposter_pubkeys: Some(reposter_pubkeys),
        },
    );
    fbb.finish(root, Some(fb::REPOST_SUMMARY_SNAPSHOT_IDENTIFIER));
    fbb.finished_data().to_vec()
}

/// Decode a [`RepostSummarySnapshot`] from its typed FlatBuffers payload —
/// the inverse of [`encode_repost_summary_snapshot`] (#2900). A pure-Rust
/// consumer that links this crate directly (no UniFFI/codegen boundary — e.g.
/// a TUI/desktop shell) uses this to turn the
/// `TypedProjectionData::payload` bytes the read-lifecycle engine emits back
/// into the typed snapshot, mirroring `nmp_core::typed_projections::
/// decode_relay_diagnostics` / `nmp_nip01::decode_op_feed_snapshot`'s
/// symmetric encode+decode convention.
///
/// # Errors
///
/// Returns a formatted `String` when `bytes` does not carry the
/// `REPOST_SUMMARY_FILE_IDENTIFIER` or fails FlatBuffers verification.
pub fn decode_repost_summary_snapshot(bytes: &[u8]) -> Result<RepostSummarySnapshot, String> {
    if bytes.len() < 8 || !fb::repost_summary_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NRPS file identifier".to_string());
    }
    let root = fb::root_as_repost_summary_snapshot(bytes)
        .map_err(|e| format!("not a valid RepostSummarySnapshot buffer: {e}"))?;
    Ok(RepostSummarySnapshot {
        target_id: root.target_id().unwrap_or_default().to_string(),
        count: root.count(),
        reposter_pubkeys: root
            .reposter_pubkeys()
            .map(|pubkeys| pubkeys.iter().map(str::to_string).collect())
            .unwrap_or_default(),
    })
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
