//! `open_zaps` — the concept-owned zap-summary active read (#2758, #2508).
//!
//! This is the zap owner's DOOR. It compiles the NIP-57 kind:9735 `#e` demand
//! for one target into demand + admission + reducer + typed output, then
//! drives it through the ONE read-lifecycle engine (`nmp-read-session`). It
//! contains NO registry, NO close map, NO replay implementation, and NO
//! teardown recipe of its own — those are the engine's, reached only via
//! [`open_read`] / [`close_read`]. If this file ever grew any of them, the
//! engine boundary would be wrong (#2777).
//!
//! The symbol lives HERE, in the concept crate: a kernel that does not import
//! `nmp-zaps` has no `open_zaps`. `open_zaps` takes the engine's host seam
//! (`&dyn ReadHost`, which a runtime like `NmpApp` implements once) — it
//! never depends on a runtime crate. Dependency direction: `nmp-zaps` →
//! `nmp-read-session` ← runtime.
//!
//! # Deletion handling (out of scope here, unlike `nmp-reposts`)
//!
//! `nmp-reposts` folds a same-author kind:5 NIP-09 delete to retract a
//! reposter's own wrapper, because the reposting pubkey is also the
//! wrapper's author. A zap receipt's author is the LN provider that minted
//! it (NIP-57 `provider_pubkey`), not the sender or the zapped target, so
//! there is no analogous "the party who would retract it is the one whose
//! kind:5 we'd fold" relationship — and receipts are essentially never
//! deleted in practice. This crate does not fold kind:5 for receipts; if a
//! future need arises, note that NIP-09 deletes name the *deleted event's
//! own id*, which a `ReadSpec` demand fixed at `open_zaps` time cannot
//! subscribe to for a receipt id not yet observed — the same static-demand
//! gap `nmp-reposts` documents for #2777.
//!
//! # Viewer-relative facts
//!
//! `total_msats`, `zap_count`, and `zappers` are the raw aggregate facts
//! (D0); whether the ACTIVE user zapped is derived by the shell comparing
//! its own already-available active-account pubkey against `zappers`' raw
//! pubkeys — this concept never depends on viewer identity, so `open_zaps`
//! takes no viewer parameter (mirrors `nmp-reposts`' `reposter_pubkeys`).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use flatbuffers::FlatBufferBuilder;
use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use nmp_ownership::FrameworkProjectionKey;
use nmp_read_session::{close_read, open_read, ReadDemand, ReadHost, ReadOutputEncoder, ReadSpec};

use crate::read::ZapReadPlan;
use crate::target::{ZapTarget, ZapTargetError};

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
#[path = "wire/generated/zap_summary_generated.rs"]
mod generated;

use generated::nmp::zaps as fb;

/// Stable schema id for the zap-summary typed projection.
pub const ZAP_SUMMARY_SCHEMA_ID: &str = "nmp.zaps.summary";
/// Schema version mirrored in `zap_summary.fbs`.
pub const ZAP_SUMMARY_SCHEMA_VERSION: u32 = 1;
/// FlatBuffers file identifier for the zap-summary buffer.
pub const ZAP_SUMMARY_FILE_IDENTIFIER: &[u8] = b"NZSM";
/// Account-agnostic scope: a note's zaps come from anywhere, routed by the
/// target's outbox rather than the viewer's account.
const ZAP_READ_SCOPE_GLOBAL: u32 = 1;
/// Bounded read-cache replay depth before live activation.
const ZAP_REPLAY_LIMIT: usize = 512;

/// Every `open_zaps` call gets a unique output key, so independent zap-count
/// components on the same target are fully separate reads (feed's
/// unique-output model), not a shared singleton.
static NEXT_ZAP_READ: AtomicU64 = AtomicU64::new(1);

/// One sender's (or the anonymous bucket's) aggregated zap total for a
/// target. Raw data only (aim.md Section 2): `pubkey` is the raw sender
/// pubkey from the receipt's validated embedded zap request, never a display
/// string.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZapperTotal {
    /// `None` for the anonymous bucket (NIP-57 permits a zap with no
    /// discoverable sender).
    pub pubkey: Option<String>,
    /// Millisats this sender (or the anonymous bucket) has sent.
    pub total_msats: u64,
    /// Distinct accepted receipts from this sender (or the anonymous
    /// bucket).
    pub zap_count: u32,
}

/// The zap-summary read model for ONE target. Raw data only (aim.md
/// Section 2) — no viewer-relative field; the shell derives "did I zap" by
/// membership-checking its own pubkey against `zappers`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZapSummarySnapshot {
    /// The raw target event id.
    pub target_id: String,
    /// Sum of `amount_msats` across every distinct accepted receipt.
    pub total_msats: u64,
    /// Distinct accepted receipts (deduplicated by receipt event id).
    pub zap_count: u64,
    /// Per-sender aggregation, ascending by pubkey (the anonymous bucket, if
    /// present, sorts first). Bounded by the reducer's input map capacity
    /// (`MAX_PROJECTION_MESSAGES`, D8).
    pub zappers: Vec<ZapperTotal>,
}

/// The admission-applying zap reducer: the concept's event fold. It ingests
/// candidate receipt events (delivered by the demand filter) and keeps only
/// those the read plan's admission ACCEPTS as validated zaps of the target,
/// deduplicated by receipt event id. This is the ONLY concept-owned stateful
/// piece — it holds no lifecycle, only the read model, and it holds no
/// viewer identity (see the module-level "Viewer-relative facts" note).
pub struct ZapSummaryProjection {
    target_id: String,
    plan: ZapReadPlan,
    // receipt event id -> (sender pubkey, amount_msats).
    accepted: Mutex<BoundedMessageMap<String, (Option<String>, u64)>>,
}

impl ZapSummaryProjection {
    #[must_use]
    fn new(target: ZapTarget) -> Self {
        let target_id = target.event_id().to_string();
        Self {
            target_id,
            plan: ZapReadPlan::new(target),
            accepted: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// The current zap summary (zappers ascending by pubkey, anonymous
    /// bucket first).
    #[must_use]
    pub fn snapshot(&self) -> ZapSummarySnapshot {
        let Ok(accepted) = self.accepted.lock() else {
            return ZapSummarySnapshot {
                target_id: self.target_id.clone(),
                ..Default::default()
            };
        };

        let mut by_sender: BTreeMap<Option<String>, (u64, u32)> = BTreeMap::new();
        let mut total_msats: u64 = 0;
        for (_, (sender, msats)) in accepted.iter() {
            total_msats += *msats;
            let bucket = by_sender.entry(sender.clone()).or_insert((0, 0));
            bucket.0 += *msats;
            bucket.1 += 1;
        }

        let zappers: Vec<ZapperTotal> = by_sender
            .into_iter()
            .map(|(pubkey, (total_msats, zap_count))| ZapperTotal {
                pubkey,
                total_msats,
                zap_count,
            })
            .collect();

        ZapSummarySnapshot {
            target_id: self.target_id.clone(),
            total_msats,
            zap_count: accepted.len() as u64,
            zappers,
        }
    }
}

impl ObservedProjectionSink for ZapSummaryProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        // Admission + protocol validation (amount consistency, known-provider
        // mismatch rejection) live in `ZapReadPlan::accepts` / `nmp-nip57`;
        // an invalid receipt is silently excluded here, never errored.
        let Some(record) = self.plan.accepts(event) else {
            return;
        };
        if let Ok(mut accepted) = self.accepted.lock() {
            accepted.insert(
                record.event_id,
                (record.sender_pubkey, record.amount_msats.unwrap_or(0)),
            );
        }
    }
}

/// The typed close handle `open_zaps` returns. Wraps the engine's opaque
/// handle so a zap read can only be closed with [`close_zaps`] (not with a
/// feed/reply/repost handle), and exposes the projection key the shell
/// renders from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZapsReadHandle(nmp_read_session::ReadHandle);

impl ZapsReadHandle {
    /// The projection key this read's typed [`ZapSummarySnapshot`] surfaces
    /// under. The shell learns it from the handle and renders that key.
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Open a live zap-summary read for the plain kind:1 note `target_event_id`
/// on the read-lifecycle engine.
///
/// Compiles the NIP-57 kind:9735 `#e` demand, folds accepted receipts (per
/// `nmp-nip57`'s own decode + validation) into one reducer, and emits a typed
/// [`ZapSummarySnapshot`]. Returns a close handle; [`close_zaps`] withdraws
/// the demand and tombstones the output.
///
/// # Errors
///
/// Returns [`ZapTargetError`] when `target_event_id` is not a 64-hex event
/// id.
pub fn open_zaps(
    host: &dyn ReadHost,
    target_event_id: impl Into<String>,
) -> Result<ZapsReadHandle, ZapTargetError> {
    let target = ZapTarget::event(target_event_id)?;
    let token = target.event_id().to_string();
    let nonce = NEXT_ZAP_READ.fetch_add(1, Ordering::Relaxed);
    let key_string = format!("nmp.zaps.summary.{token}.{nonce}");

    let plan = ZapReadPlan::new(target.clone());
    let demand = ReadDemand {
        filter_json: plan.filter_json(),
        consumer_id: key_string.clone(),
        scope: ZAP_READ_SCOPE_GLOBAL,
        relay_pin: None,
        replay_limit: ZAP_REPLAY_LIMIT,
    };

    let projection = Arc::new(ZapSummaryProjection::new(target));

    // Typed output: encode the reducer's snapshot each tick. Coalesced
    // emission + tombstone-on-close are the engine/runtime's, not this
    // closure's.
    let projection_for_output = Arc::clone(&projection);
    let output_key = key_string.clone();
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.snapshot();
        Some(TypedProjectionData {
            key: output_key.clone(),
            schema_id: ZAP_SUMMARY_SCHEMA_ID.to_string(),
            schema_version: ZAP_SUMMARY_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(ZAP_SUMMARY_FILE_IDENTIFIER).into_owned(),
            payload: encode_zap_summary_snapshot(&snapshot),
            ..Default::default()
        })
    });

    // `nmp.zaps.*` is a framework prefix, so this declaration cannot fail.
    // The owner-claim literal must appear here for the crate-ownership audit.
    let projection_key = FrameworkProjectionKey::declared(key_string, "projection.nmp.zaps.summary")
        .expect("nmp.zaps.summary.* carries the framework prefix");

    let handle = open_read(
        host,
        ReadSpec {
            projection_key: projection_key.into(),
            demands: vec![demand],
            observer: projection as Arc<dyn ObservedProjectionSink>,
            output_encoder,
        },
    );
    Ok(ZapsReadHandle(handle))
}

/// Close a zap read opened by [`open_zaps`], withdrawing the demand and
/// tombstoning the typed output. Idempotent (D6).
pub fn close_zaps(host: &dyn ReadHost, handle: ZapsReadHandle) -> bool {
    close_read(host, &handle.0)
}

/// Encode a [`ZapSummarySnapshot`] to its typed FlatBuffers payload.
#[must_use]
pub fn encode_zap_summary_snapshot(snapshot: &ZapSummarySnapshot) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let target_id = fbb.create_string(&snapshot.target_id);
    let zapper_offsets: Vec<_> = snapshot
        .zappers
        .iter()
        .map(|z| {
            let pubkey = z.pubkey.as_deref().map(|s| fbb.create_string(s));
            fb::ZapperTotal::create(
                &mut fbb,
                &fb::ZapperTotalArgs {
                    pubkey,
                    total_msats: z.total_msats,
                    zap_count: z.zap_count,
                },
            )
        })
        .collect();
    let zappers = fbb.create_vector(&zapper_offsets);
    let root = fb::ZapSummarySnapshot::create(
        &mut fbb,
        &fb::ZapSummarySnapshotArgs {
            schema_version: ZAP_SUMMARY_SCHEMA_VERSION,
            target_id: Some(target_id),
            total_msats: snapshot.total_msats,
            zap_count: snapshot.zap_count,
            zappers: Some(zappers),
        },
    );
    fbb.finish(root, Some(fb::ZAP_SUMMARY_SNAPSHOT_IDENTIFIER));
    fbb.finished_data().to_vec()
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
