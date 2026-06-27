//! Shared derivation for the `refs.event.envelopes` render projection.

use std::collections::BTreeMap;

use nmp_core::refs::RefEventStore;
use nmp_core::substrate::KernelEvent;
use nmp_core::typed_projections::ClaimedEventRow;

use crate::context::RenderContext;

use super::{resolve_embed_projection, EmbeddedEventEnvelope, RenderContextWire};

/// Derive the render-facing `refs.event.envelopes` map from a merged
/// `refs.event` row store.
///
/// `refs.event` remains the authoritative event-reference row source. This
/// function is the reusable composition path that converts the materialised row
/// set into `primary_id -> EmbeddedEventEnvelope` values through
/// [`resolve_embed_projection`]. Malformed hand-assembled rows fail closed by
/// being skipped.
#[must_use]
pub fn derive_ref_event_envelopes(
    rows: &BTreeMap<String, ClaimedEventRow>,
) -> BTreeMap<String, EmbeddedEventEnvelope> {
    let ctx = RenderContext::new();
    derive_ref_event_envelopes_with_context(rows, &ctx)
}

/// Derive the render-facing `refs.event.envelopes` map directly from a
/// [`RefEventStore`].
#[must_use]
pub fn derive_ref_event_store_envelopes(
    store: &RefEventStore,
) -> BTreeMap<String, EmbeddedEventEnvelope> {
    derive_ref_event_envelopes(&store.events())
}

fn derive_ref_event_envelopes_with_context(
    rows: &BTreeMap<String, ClaimedEventRow>,
    ctx: &RenderContext,
) -> BTreeMap<String, EmbeddedEventEnvelope> {
    rows.iter()
        .filter_map(|(primary_id, row)| {
            let event = row_to_kernel_event(primary_id, row)?;
            let projection = resolve_embed_projection(&event, ctx);
            Some((
                primary_id.clone(),
                EmbeddedEventEnvelope {
                    uri: String::new(),
                    primary_id: primary_id.clone(),
                    render_context: RenderContextWire::from(ctx),
                    projection,
                    collapsed: false,
                    collapse_reason: None,
                },
            ))
        })
        .collect()
}

fn row_to_kernel_event(primary_id: &str, row: &ClaimedEventRow) -> Option<KernelEvent> {
    if primary_id.is_empty()
        || row.primary_id != primary_id
        || row.id.is_empty()
        || row.author_pubkey.is_empty()
    {
        return None;
    }
    Some(KernelEvent {
        id: row.id.clone(),
        author: row.author_pubkey.clone(),
        kind: row.kind,
        created_at: row.created_at,
        tags: row.tags.clone(),
        content: row.content.clone(),
        relay_provenance: Vec::new(),
    })
}
