//! `open_replies` — the concept-owned reply-count active read (#2758, #2508).
//!
//! This is the reply owner's DOOR. It composes the applicable reply conventions
//! (NIP-10 kind:1 `#e`, NIP-22 kind:1111 `#E`/`#e`) into demand + admission +
//! reducer + typed output, then drives them through the ONE read-lifecycle
//! engine (`nmp-read-session`). It contains NO registry, NO close map, NO replay
//! implementation, and NO teardown recipe of its own — those are the engine's,
//! reached only via [`open_read`] / [`close_read`]. If this file ever grew any
//! of them, the engine boundary would be wrong (#2777).
//!
//! The symbol lives HERE, in the concept crate: a kernel that does not import
//! `nmp-replies` has no `open_replies`. `open_replies` takes the engine's host
//! seam (`&dyn ReadHost`, which a runtime like `NmpApp` implements once) — it
//! never depends on a runtime crate. Dependency direction: `nmp-replies` →
//! `nmp-read-session` ← runtime.
//!
//! # Deletion handling
//!
//! A reply author's own kind:5 NIP-09 delete retracts that reply. NIP-09 names
//! the reply event id, so this crate declares the current delete shape from its
//! accepted reply ids/authors while the read-session engine owns the dynamic
//! observed-demand lifecycle.

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
    close_read, open_read, ReadDemand, ReadDependentDemand, ReadDependentDemandProvider, ReadHost,
    ReadOutputEncoder, ReadReplayPolicy, ReadSpec,
};
use serde::{Deserialize, Serialize};

use crate::read::{reply_read_plans, ReplyReadPlan, ReplyReadPlanError};
use crate::target::ReplyTarget;

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
#[path = "wire/generated/reply_summary_generated.rs"]
mod generated;

use generated::nmp::replies as fb;

/// Stable schema id for the reply-summary typed projection.
pub const REPLY_SUMMARY_SCHEMA_ID: &str = "nmp.replies.summary";
/// Schema version mirrored in `reply_summary.fbs`.
pub const REPLY_SUMMARY_SCHEMA_VERSION: u32 = 1;
/// FlatBuffers file identifier for the reply-summary buffer.
pub const REPLY_SUMMARY_FILE_IDENTIFIER: &[u8] = b"NRSM";
/// Account-agnostic scope: a note's replies come from anywhere, routed by the
/// target's outbox rather than the viewer's account.
const REPLY_READ_SCOPE_GLOBAL: u32 = 1;
/// Bounded read-cache replay depth per demand before live activation.
const REPLY_REPLAY_LIMIT: usize = 512;

/// Every `open_replies` call gets a unique output key, so independent reply-count
/// components on the same target are fully separate reads (feed's unique-output
/// model), not a shared singleton.
static NEXT_REPLY_READ: AtomicU64 = AtomicU64::new(1);

/// The reply-summary read model for ONE target: distinct accepted replies +
/// their raw ids. Raw data only (aim.md §2).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReplySummarySnapshot {
    /// The raw target identifier (event id / coordinate / URI).
    pub target_id: String,
    /// Distinct accepted replies across every applicable convention.
    pub count: u64,
    /// Raw hex ids of the accepted reply events, ascending.
    pub reply_event_ids: Vec<String>,
}

/// The admission-applying reply reducer: the concept's event fold. It ingests
/// candidate reply events (delivered by the demand filters) and keeps only those
/// its plans ACCEPT as true replies to the target (a bare `#e` mention is not a
/// reply), deduplicated by event id. This is the ONLY concept-owned stateful
/// piece — it holds no lifecycle, only the read model.
pub struct ReplySummaryProjection {
    target_id: String,
    plans: Vec<ReplyReadPlan>,
    // reply event id -> reply author pubkey.
    accepted: Mutex<BoundedMessageMap<String, String>>,
}

impl ReplySummaryProjection {
    #[must_use]
    fn new(target_id: String, plans: Vec<ReplyReadPlan>) -> Self {
        Self {
            target_id,
            plans,
            accepted: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// The current reply summary (ids ascending, count = distinct accepted).
    #[must_use]
    pub fn snapshot(&self) -> ReplySummarySnapshot {
        let Ok(accepted) = self.accepted.lock() else {
            return ReplySummarySnapshot {
                target_id: self.target_id.clone(),
                ..Default::default()
            };
        };
        let mut reply_event_ids: Vec<String> = accepted.iter().map(|(id, _)| id.clone()).collect();
        reply_event_ids.sort();
        ReplySummarySnapshot {
            target_id: self.target_id.clone(),
            count: reply_event_ids.len() as u64,
            reply_event_ids,
        }
    }

    fn ingest_delete(&self, event: &KernelEvent) {
        let deleted_ids = nmp_nip09::DeleteRecord::try_from_kernel_event(event)
            .map(|record| record.event_targets)
            .unwrap_or_default();
        if deleted_ids.is_empty() {
            return;
        }
        if let Ok(mut accepted) = self.accepted.lock() {
            for id in deleted_ids {
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
        let reply_ids = accepted
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<BTreeSet<_>>();
        let mut tags = BTreeMap::new();
        tags.insert("e".to_string(), reply_ids);
        Some(ReadDependentDemand {
            shape: InterestShape {
                authors: accepted.values().cloned().collect::<BTreeSet<_>>(),
                kinds: BTreeSet::from([KIND_DELETION]),
                tags,
                ..Default::default()
            },
            scope: REPLY_READ_SCOPE_GLOBAL,
            is_indexer_discovery: false,
            replay_limit: REPLY_REPLAY_LIMIT,
        })
    }
}

impl ObservedProjectionSink for ReplySummaryProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind == KIND_DELETION {
            self.ingest_delete(event);
            return;
        }
        // Admission: only true replies to the target (any applicable
        // convention) are counted; the demand filter is a superset.
        if self.plans.iter().any(|plan| plan.accepts(event)) {
            if let Ok(mut accepted) = self.accepted.lock() {
                accepted.insert(event.id.clone(), event.author.clone());
            }
        }
    }
}

/// The typed close handle `open_replies` returns. Wraps the engine's opaque
/// handle so a reply read can only be closed with [`close_replies`] (not with a
/// feed handle), and exposes the projection key the shell renders from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepliesReadHandle(nmp_read_session::ReadHandle);

impl RepliesReadHandle {
    /// The projection key this read's typed [`ReplySummarySnapshot`] surfaces
    /// under. The shell learns it from the handle and renders that key.
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Open a live reply-count read for `target` on the read-lifecycle engine.
///
/// Composes the applicable conventions (NIP-10 kind:1 and/or NIP-22 kind:1111)
/// into one read: several routed demands folding into one admission-applying
/// reducer that emits a typed [`ReplySummarySnapshot`]. Returns a close handle;
/// [`close_replies`] withdraws every demand and tombstones the output.
///
/// # Errors
///
/// Returns [`ReplyReadPlanError`] if a target cannot be compiled to a plan (e.g.
/// a kind:1111 event target passed without its decoded `CommentRecord`).
pub fn open_replies(
    host: &dyn ReadHost,
    target: ReplyTarget,
) -> Result<RepliesReadHandle, ReplyReadPlanError> {
    let plans = reply_read_plans(&target)?;
    let token = target.summary_token();
    let nonce = NEXT_REPLY_READ.fetch_add(1, Ordering::Relaxed);
    let key_string = format!("nmp.replies.summary.{token}.{nonce}");

    // Demand: one live `REQ` per applicable convention, all folding into one
    // reducer that also owns admission for those same conventions.
    let demands: Vec<ReadDemand> = plans
        .iter()
        .map(|plan| ReadDemand {
            filter_json: plan.filter_json(),
            consumer_id: key_string.clone(),
            scope: REPLY_READ_SCOPE_GLOBAL,
            relay_pin: None,
            is_indexer_discovery: false,
            replay_limit: REPLY_REPLAY_LIMIT,
            replay: ReadReplayPolicy::Structural,
        })
        .collect();

    let projection = Arc::new(ReplySummaryProjection::new(token, plans));
    let dependent_demands: Vec<ReadDependentDemandProvider> = {
        let projection = Arc::clone(&projection);
        vec![Arc::new(move || projection.delete_demand())]
    };

    // Typed output: encode the reducer's snapshot each tick. Coalesced emission
    // + tombstone-on-close are the engine/runtime's, not this closure's.
    let projection_for_output = Arc::clone(&projection);
    let output_key = key_string.clone();
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.snapshot();
        Some(TypedProjectionData {
            key: output_key.clone(),
            schema_id: REPLY_SUMMARY_SCHEMA_ID.to_string(),
            schema_version: REPLY_SUMMARY_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(REPLY_SUMMARY_FILE_IDENTIFIER).into_owned(),
            payload: encode_reply_summary_snapshot(&snapshot),
            ..Default::default()
        })
    });

    // `nmp.replies.*` is a framework prefix, so this declaration cannot fail.
    // The owner-claim literal must appear here for the crate-ownership audit.
    let projection_key =
        FrameworkProjectionKey::declared(key_string, "projection.nmp.replies.summary")
            .expect("nmp.replies.summary.* carries the framework prefix");

    let handle = open_read(
        host,
        ReadSpec {
            projection_key: projection_key.into(),
            demands,
            observer: projection as Arc<dyn ObservedProjectionSink>,
            output_encoder,
            dependent_demands,
            keep_open_without_live_demand: false,
        },
    );
    Ok(RepliesReadHandle(handle))
}

/// Close a reply read opened by [`open_replies`], withdrawing every demand and
/// tombstoning the typed output (reverse order, once). Idempotent (D6).
pub fn close_replies(host: &dyn ReadHost, handle: RepliesReadHandle) -> bool {
    close_read(host, &handle.0)
}

/// Encode a [`ReplySummarySnapshot`] to its typed FlatBuffers payload.
#[must_use]
pub fn encode_reply_summary_snapshot(snapshot: &ReplySummarySnapshot) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let target_id = fbb.create_string(&snapshot.target_id);
    let id_offsets: Vec<_> = snapshot
        .reply_event_ids
        .iter()
        .map(|id| fbb.create_string(id))
        .collect();
    let reply_event_ids = fbb.create_vector(&id_offsets);
    let root = fb::ReplySummarySnapshot::create(
        &mut fbb,
        &fb::ReplySummarySnapshotArgs {
            schema_version: REPLY_SUMMARY_SCHEMA_VERSION,
            target_id: Some(target_id),
            count: snapshot.count,
            reply_event_ids: Some(reply_event_ids),
        },
    );
    fbb.finish(root, Some(fb::REPLY_SUMMARY_SNAPSHOT_IDENTIFIER));
    fbb.finished_data().to_vec()
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
