//! `open_reactions` — the concept-owned reaction-count active read (#2758, #2508).
//!
//! This is the reaction owner's DOOR. It composes ONE routed demand (kind:7
//! reactions + their kind:5 NIP-09 retractions, `#e`-tagging the target) with
//! `nmp_nip25::ReactionAggregateProjection` — the SAME kind:7/kind:5 fold the
//! NIP-29-group-scoped reaction read already drives — as the admission +
//! reducer, then drives them through the ONE read-lifecycle engine
//! (`nmp-read-session`). It contains NO registry, NO close map, NO replay
//! implementation, and NO teardown recipe of its own — those are the
//! engine's, reached only via [`open_read`] / [`close_read`]. If this file
//! ever grew any of them, the engine boundary would be wrong (#2777).
//!
//! The symbol lives HERE, in the concept crate: a kernel that does not import
//! `nmp-reactions` has no `open_reactions`. `open_reactions` takes the
//! engine's host seam (`&dyn ReadHost`, which a runtime like `NmpApp`
//! implements once) — it never depends on a runtime crate. Dependency
//! direction: `nmp-reactions` -> `nmp-read-session` <- runtime.
//!
//! # Deletion handling
//!
//! A reacting pubkey's own kind:5 NIP-09 delete retracts their reaction —
//! `ReactionAggregateProjection::ingest` decodes deletes through
//! `nmp_nip09::DeleteRecord` (the canonical delete read seam, ADR-0074) and
//! removes a stored reaction only when the delete's author matches the
//! original reactor. That machinery is reused unchanged; nothing here
//! reimplements retraction. Because NIP-09 names the *deleted event's own id*
//! in its `e` tags (the kind:7 reaction's id, not the target's), and the
//! reaction's id is only known once the reaction itself has been observed, a
//! demand fixed at `open_reactions` time can only route a delete of an
//! *already-known* reaction if the deleting client also happens to co-tag the
//! target — the reducer's retraction logic is correct and independently
//! tested, but full live-retraction of an arbitrary stranger's reaction needs
//! a demand that can grow as new reaction ids are discovered, which the
//! current [`nmp_read_session::ReadSpec`] (fixed at open time) does not
//! support. That is a real boundary gap for #2777 (the same gap `nmp-reposts`
//! documents), not something this crate should paper over with a private
//! per-id re-subscription loop. The pre-engine `NmpApp::open_reactions` lane
//! uses `ObservedProjectionReconciler` for that dynamic delete demand today;
//! making it an engine-supplied stage is future engine work.
//! (The NIP-29 group lane does not have this gap because in-group deletes
//! carry the `#h` envelope tag its filter matches.)
//!
//! # Viewer-relative facts
//!
//! `total`, per-group `count`, and per-group `reactor_pubkeys` are the raw
//! distinct-reactor facts (D0); whether the ACTIVE user reacted — and with
//! which token — is derived by the shell comparing its own already-available
//! active-account pubkey against a group's `reactor_pubkeys` — this concept
//! never depends on viewer identity, so `open_reactions` takes no viewer
//! parameter. This deliberately diverges from the NIP-29 group lane's
//! snapshot (which carries a viewer-scoped `mine` list): the identity-free
//! shape is the #2758 concept-read precedent (`nmp-reposts`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use flatbuffers::FlatBufferBuilder;
use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use nmp_nip25::ReactionAggregateProjection;
use nmp_ownership::FrameworkProjectionKey;
use nmp_read_session::{close_read, open_read, ReadDemand, ReadHost, ReadOutputEncoder, ReadSpec};
use serde::{Deserialize, Serialize};

use crate::read::reaction_filter_json;
use crate::target::{ReactionTarget, ReactionTargetError};

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
#[path = "wire/generated/reaction_summary_generated.rs"]
mod generated;

use generated::nmp::reactions as fb;

/// Stable schema id for the reaction-summary typed projection.
pub const REACTION_SUMMARY_SCHEMA_ID: &str = "nmp.reactions.summary";
/// Schema version mirrored in `reaction_summary.fbs`.
pub const REACTION_SUMMARY_SCHEMA_VERSION: u32 = 1;
/// FlatBuffers file identifier for the reaction-summary buffer.
pub const REACTION_SUMMARY_FILE_IDENTIFIER: &[u8] = b"NRCS";
/// Account-agnostic scope: a note's reactions come from anywhere, routed by
/// the target's outbox rather than the viewer's account (mirrors
/// `nmp-replies`' `REPLY_READ_SCOPE_GLOBAL`).
const REACTION_READ_SCOPE_GLOBAL: u32 = 1;
/// Bounded read-cache replay depth before live activation.
const REACTION_REPLAY_LIMIT: usize = 512;

/// Every `open_reactions` call gets a unique output key, so independent
/// reaction-count components on the same target are fully separate reads
/// (feed's unique-output model), not a shared singleton.
static NEXT_REACTION_READ: AtomicU64 = AtomicU64::new(1);

/// One reaction-content group's tally within a [`ReactionSummarySnapshot`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReactionGroupSummary {
    /// Raw reaction content token ("+" for empty/like; emoji; NIP-30 shortcode).
    pub token: String,
    /// Surviving reactions carrying this token for the target (exact).
    pub count: u64,
    /// Distinct reactor pubkeys for this token (raw hex), ascending. The
    /// shell derives "did the active user react with this token" by raw
    /// membership against its own active-account pubkey. Bounded by the
    /// reducer's ingest map (see module docs), never a lossy sample.
    pub reactor_pubkeys: Vec<String>,
}

/// The reaction-summary read model for ONE target: total + per-content-token
/// groups, each with its distinct reactor pubkeys. Raw data only (aim.md
/// Section 2) — no viewer-relative field is computed here; `open_reactions`
/// takes no viewer parameter, so a caller that already knows its own active
/// pubkey tests membership in a group's `reactor_pubkeys` itself.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReactionSummarySnapshot {
    /// The reacted-to event id (raw hex).
    pub target_id: String,
    /// Total surviving reactions across all content tokens (exact).
    pub total: u64,
    /// Per-content-token breakdown (count desc, then token asc).
    pub groups: Vec<ReactionGroupSummary>,
}

/// Fold the reducer's current per-target aggregate + per-token reactor
/// membership into the typed snapshot. Pure read-model assembly — both inputs
/// come from the same `nmp-nip25` fold, so counts and membership always agree.
fn reaction_summary_for(
    projection: &ReactionAggregateProjection,
    target_id: &str,
) -> ReactionSummarySnapshot {
    let Some(aggregate) = projection.aggregate_for(target_id) else {
        return ReactionSummarySnapshot {
            target_id: target_id.to_string(),
            ..Default::default()
        };
    };
    let mut reactors_by_token = projection.reactors_by_token_for(target_id);
    ReactionSummarySnapshot {
        target_id: aggregate.target_event_id,
        total: aggregate.total,
        groups: aggregate
            .by_emoji
            .into_iter()
            .map(|emoji| ReactionGroupSummary {
                reactor_pubkeys: reactors_by_token.remove(&emoji.token).unwrap_or_default(),
                token: emoji.token,
                count: emoji.count,
            })
            .collect(),
    }
}

/// The typed close handle `open_reactions` returns. Wraps the engine's opaque
/// handle so a reaction read can only be closed with [`close_reactions`] (not
/// with a feed or replies handle), and exposes the projection key the shell
/// renders from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionsReadHandle(nmp_read_session::ReadHandle);

impl ReactionsReadHandle {
    /// The projection key this read's typed [`ReactionSummarySnapshot`]
    /// surfaces under. The shell learns it from the handle and renders that
    /// key.
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Open a live reaction-count read for `target_event_id` on the
/// read-lifecycle engine.
///
/// Composes ONE routed demand (kind:7 + kind:5, `#e`-tagging the target) with
/// `nmp_nip25::ReactionAggregateProjection` as the admission-applying reducer,
/// then drives it through the engine. Returns a close handle;
/// [`close_reactions`] withdraws the demand and tombstones the output.
///
/// # Errors
///
/// Returns [`ReactionTargetError`] when `target_event_id` is not a 64-hex
/// event id.
pub fn open_reactions(
    host: &dyn ReadHost,
    target_event_id: impl Into<String>,
) -> Result<ReactionsReadHandle, ReactionTargetError> {
    let target = ReactionTarget::event(target_event_id)?;
    let token = target.as_str().to_string();
    let nonce = NEXT_REACTION_READ.fetch_add(1, Ordering::Relaxed);
    let key_string = format!("nmp.reactions.summary.{token}.{nonce}");

    // Demand: one live `REQ` (kind:7 + kind:5, `#e`-tagged) folding into the
    // NIP-25 aggregate reducer, which also owns retraction admission.
    let demand = ReadDemand {
        filter_json: reaction_filter_json(&target),
        consumer_id: key_string.clone(),
        scope: REACTION_READ_SCOPE_GLOBAL,
        relay_pin: None,
        replay_limit: REACTION_REPLAY_LIMIT,
    };

    // No viewer pubkey: this read is identity-free (module docs), so the
    // aggregate's viewer-scoped `mine` stays disabled; shells derive
    // viewer-relative facts from each group's raw `reactor_pubkeys`.
    let projection = Arc::new(ReactionAggregateProjection::new(None));

    // Typed output: encode the reducer's per-target summary each tick.
    // Coalesced emission + tombstone-on-close are the engine/runtime's, not
    // this closure's.
    let projection_for_output = Arc::clone(&projection);
    let output_target = token.clone();
    let output_key = key_string.clone();
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = reaction_summary_for(&projection_for_output, &output_target);
        Some(TypedProjectionData {
            key: output_key.clone(),
            schema_id: REACTION_SUMMARY_SCHEMA_ID.to_string(),
            schema_version: REACTION_SUMMARY_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(REACTION_SUMMARY_FILE_IDENTIFIER).into_owned(),
            payload: encode_reaction_summary_snapshot(&snapshot),
            ..Default::default()
        })
    });

    // `nmp.reactions.*` is a framework prefix, so this declaration cannot
    // fail. The owner-claim literal must appear here for the crate-ownership
    // audit.
    let projection_key =
        FrameworkProjectionKey::declared(key_string, "projection.nmp.reactions.summary")
            .expect("nmp.reactions.summary.* carries the framework prefix");

    let handle = open_read(
        host,
        ReadSpec {
            projection_key: projection_key.into(),
            demands: vec![demand],
            observer: projection as Arc<dyn ObservedProjectionSink>,
            output_encoder,
        },
    );
    Ok(ReactionsReadHandle(handle))
}

/// Close a reaction read opened by [`open_reactions`], withdrawing the demand
/// and tombstoning the typed output. Idempotent (D6).
pub fn close_reactions(host: &dyn ReadHost, handle: ReactionsReadHandle) -> bool {
    close_read(host, &handle.0)
}

/// Encode a [`ReactionSummarySnapshot`] to its typed FlatBuffers payload.
#[must_use]
pub fn encode_reaction_summary_snapshot(snapshot: &ReactionSummarySnapshot) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let target_id = fbb.create_string(&snapshot.target_id);

    let group_offsets: Vec<_> = snapshot
        .groups
        .iter()
        .map(|group| {
            let token = fbb.create_string(&group.token);
            let reactor_offsets: Vec<_> = group
                .reactor_pubkeys
                .iter()
                .map(|pubkey| fbb.create_string(pubkey))
                .collect();
            let reactor_pubkeys = fbb.create_vector(&reactor_offsets);
            fb::ReactionGroupSummary::create(
                &mut fbb,
                &fb::ReactionGroupSummaryArgs {
                    token: Some(token),
                    count: group.count,
                    reactor_pubkeys: Some(reactor_pubkeys),
                },
            )
        })
        .collect();
    let groups = fbb.create_vector(&group_offsets);

    let root = fb::ReactionSummarySnapshot::create(
        &mut fbb,
        &fb::ReactionSummarySnapshotArgs {
            schema_version: REACTION_SUMMARY_SCHEMA_VERSION,
            target_id: Some(target_id),
            total: snapshot.total,
            groups: Some(groups),
        },
    );
    fbb.finish(root, Some(fb::REACTION_SUMMARY_SNAPSHOT_IDENTIFIER));
    fbb.finished_data().to_vec()
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
