//! The single-lane `FeedParams` path's default composite lane set (#3092).
//!
//! Collapses the demolished `nmp-note-feed` crate's
//! `feed_row_builder`/`timeline_merge` pair onto the SAME
//! `composite_compiler::build_composite_rows`/`composite_merge` machinery the
//! multi-lane `open_composite_feed` path uses. Unlike a real composite feed,
//! the single-lane path has exactly ONE already-resolved acquisition source —
//! `resolve::resolve_scope` compiled ONE combined `admission` predicate that
//! already gates every acquisition kind (primary content + derived repost
//! wrapper + delete) — so this needs none of `open_composite_feed`'s per-lane
//! acquisition resolution, only its row-building/merge primitives, with both
//! lanes sharing that SAME admission predicate.
//!
//! Two lanes, kind-dispatched (never protocol-dispatched — the compiler
//! itself stays D0-clean, it only reads which KINDS a lane matches):
//!   1. `feed.authored` (`nmp_feed`'s own framework identity mapping) over the
//!      app's declared primary kinds;
//!   2. `nmp_nip18::nip18_target_render_only_mapping` over whatever
//!      repost-wrapper kind(s) (`6`/`16`) the primary-kind validator derived
//!      into the acquisition set — empty (no second lane) for a primary-kind
//!      set that derives none (e.g. a NIP-29 group timeline).

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_feed::{
    FeedRow, FlatFeedItemBuilder, FlatFeedMerge, LaneMappingId, LaneMappingRegistry, RootAdmission,
    SortPolicy, DIRECT_MAPPING_ID,
};

use crate::composite_compiler::{build_composite_rows, composite_merge, CompiledLane};
use crate::delivered_ref::DeliveredRefDemand;
use crate::source::RowContextProvider;

/// Compile the single-lane path's fixed default lane set into the same
/// `FlatFeedItemBuilder`/`FlatFeedMerge` knob pair the `Flat` shape branch
/// hands to `flat_session::build_flat_scope_session`.
///
/// `pub` (re-exported at the crate root) so a test — or an advanced caller
/// that already has its own resolved `admission`/`row_context` and wants the
/// EXACT default single-lane knob pair `open_feed` uses without driving the
/// full `FeedSessionHost` acquisition machinery — can build the identical
/// `FlatFeed` this crate's own `open_feed(FeedParams { shape: Flat, .. })`
/// path builds. This is the ONE place the default lane set is assembled.
#[must_use]
pub fn compile_default_lanes(
    admission: RootAdmission,
    row_context: RowContextProvider,
    primary_kinds: &BTreeSet<u32>,
    acquisition_kinds: &BTreeSet<u32>,
) -> (FlatFeedItemBuilder<FeedRow>, FlatFeedMerge<FeedRow>) {
    let direct_mapping = LaneMappingRegistry::new()
        .get(&LaneMappingId(DIRECT_MAPPING_ID.to_string()))
        .expect("nmp-feed pre-installs its own feed.authored mapping");

    let mut lanes = vec![CompiledLane {
        admission: admission.clone(),
        match_kinds: primary_kinds.clone(),
        match_tags: Default::default(),
        mapping: direct_mapping,
    }];

    // The repost-wrapper kind(s) (6/16) the primary-kind validator derived
    // into the acquisition set, minus the delete kind — i.e. everything in
    // `acquisition_kinds` that is NOT a primary content kind.
    let wrapper_kinds: BTreeSet<u32> = acquisition_kinds
        .difference(primary_kinds)
        .copied()
        .filter(|kind| *kind != nmp_nip18::KIND_DELETE)
        .collect();
    if !wrapper_kinds.is_empty() {
        lanes.push(CompiledLane {
            admission,
            match_kinds: wrapper_kinds,
            match_tags: Default::default(),
            mapping: nmp_nip18::nip18_target_render_only_mapping(),
        });
    }

    let lanes = Arc::new(lanes);
    // No `Delivered` ref is ever declared on this path (the render-only
    // mapping above declares `RenderOnly` refs exclusively), so this demand
    // primitive stays permanently empty — it exists only because
    // `build_composite_rows` takes one, the same signature every lane-mapping
    // caller shares.
    let demand = DeliveredRefDemand::new();
    let item_builder: FlatFeedItemBuilder<FeedRow> = {
        let lanes = Arc::clone(&lanes);
        Arc::new(move |event: &KernelEvent| {
            let mut rows = build_composite_rows(&lanes, &demand, &[], event);
            if let Some(group) = row_context(event) {
                for row in &mut rows {
                    row.card.context.push(group.clone());
                }
            }
            rows
        })
    };
    let merge = composite_merge(SortPolicy::ByInteractionTime);
    (item_builder, merge)
}

#[cfg(test)]
#[path = "flat_lane_set_tests.rs"]
mod tests;
