//! Typed FlatBuffers wire codec for the kernel-owned `"thread_view"` projection
//! (Tier-2 built-in), plus the shared [`TimelineItemModel`] row type the
//! `author_view` codec reuses.
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"thread_view"`: the serialisation of `thread_view()` (a `ThreadViewPayload`)
//! — **but only when a thread view is open** (D5: the key is OMITTED otherwise,
//! never `null`). This module adds a **typed FlatBuffers** encoding of the same
//! shape, carried in the `typed_projections` sidecar (ADR-0037) ALONGSIDE —
//! never replacing — the generic `Value` projection, and the typed entry is
//! likewise pushed only when the view is open.
//!
//! [`ThreadViewModel`] is built from the same `thread_view()` output the JSON
//! path serialises, in the same tick, so the two wire forms cannot diverge.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input.

// The generated FlatBuffers bindings are intrinsically `unsafe`. This `allow`
// block scopes the relaxation to the single generated module.
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
#[path = "generated/thread_view_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;
// Shared `TimelineItem` row type: `thread_view.fbs` `include`s `timeline_item.fbs`,
// so `TimelineItem` / `TimelineItemArgs` live in the crate-root
// `timeline_item_generated` wrapper (NOT in `fb` after the include refactor).
use crate::timeline_item_generated as ti;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const THREAD_VIEW_SCHEMA_ID: &str = "thread_view";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const THREAD_VIEW_FILE_IDENTIFIER: &[u8; 4] = b"KTVW";
/// Wire schema version. Bump on any breaking change to `thread_view.fbs`.
pub const THREAD_VIEW_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of one [`TimelineItem`](crate::kernel) — the shared
/// row type the `thread_view` and `author_view` codecs both encode (into their
/// own generated `TimelineItem` table). `Option<String>` fields are encoded as
/// `has_x` + value.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TimelineItemModel {
    pub id: String,
    pub author_pubkey: String,
    pub author_picture_url: Option<String>,
    pub author_lnurl: Option<String>,
    pub author_display_name: Option<String>,
    pub kind: u32,
    pub content: String,
    pub content_preview: String,
    pub created_at: u64,
    pub relay_count: u32,
    pub is_repost: bool,
    pub nav_target_id: String,
    pub repost_inner_content: String,
}

/// The `"thread_view"` read model — a field-for-field mirror of
/// `ThreadViewPayload`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ThreadViewModel {
    pub focused_event_id: String,
    pub root_event_id: String,
    pub state: String,
    pub items: Vec<TimelineItemModel>,
    pub previous_count: u64,
    pub next_count: u64,
    pub previous_count_label: String,
    pub next_count_label: String,
}

// --- encode ---------------------------------------------------------------

/// Encode a `[TimelineItemModel]` slice into this module's generated
/// `TimelineItem` table offsets. Shared shape with `author_view` (each codec
/// calls its OWN generated `create`).
pub(crate) fn create_timeline_items<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    items: &[TimelineItemModel],
) -> WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<ti::TimelineItem<'a>>>> {
    let offsets: Vec<WIPOffset<ti::TimelineItem<'a>>> = items
        .iter()
        .map(|item| {
            let id = fbb.create_string(&item.id);
            let author_pubkey = fbb.create_string(&item.author_pubkey);
            let author_picture_url = item
                .author_picture_url
                .as_ref()
                .map(|value| fbb.create_string(value));
            let author_lnurl = item
                .author_lnurl
                .as_ref()
                .map(|value| fbb.create_string(value));
            let author_display_name = item
                .author_display_name
                .as_ref()
                .map(|value| fbb.create_string(value));
            let content = fbb.create_string(&item.content);
            let content_preview = fbb.create_string(&item.content_preview);
            let nav_target_id = fbb.create_string(&item.nav_target_id);
            let repost_inner_content = fbb.create_string(&item.repost_inner_content);
            ti::TimelineItem::create(
                fbb,
                &ti::TimelineItemArgs {
                    id: Some(id),
                    author_pubkey: Some(author_pubkey),
                    has_author_picture_url: item.author_picture_url.is_some(),
                    author_picture_url,
                    has_author_lnurl: item.author_lnurl.is_some(),
                    author_lnurl,
                    has_author_display_name: item.author_display_name.is_some(),
                    author_display_name,
                    kind: item.kind,
                    content: Some(content),
                    content_preview: Some(content_preview),
                    created_at: item.created_at,
                    relay_count: item.relay_count,
                    is_repost: item.is_repost,
                    nav_target_id: Some(nav_target_id),
                    repost_inner_content: Some(repost_inner_content),
                },
            )
        })
        .collect();
    fbb.create_vector(&offsets)
}

/// Encode a [`ThreadViewModel`] to typed FlatBuffers bytes (with the `KTVW`
/// file identifier).
#[must_use]
pub(crate) fn encode_thread_view(model: &ThreadViewModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let focused_event_id = fbb.create_string(&model.focused_event_id);
    let root_event_id = fbb.create_string(&model.root_event_id);
    let state = fbb.create_string(&model.state);
    let items = create_timeline_items(&mut fbb, &model.items);
    let previous_count_label = fbb.create_string(&model.previous_count_label);
    let next_count_label = fbb.create_string(&model.next_count_label);
    let root = fb::ThreadViewSnapshot::create(
        &mut fbb,
        &fb::ThreadViewSnapshotArgs {
            focused_event_id: Some(focused_event_id),
            root_event_id: Some(root_event_id),
            state: Some(state),
            items: Some(items),
            previous_count: model.previous_count,
            next_count: model.next_count,
            previous_count_label: Some(previous_count_label),
            next_count_label: Some(next_count_label),
        },
    );
    fb::finish_thread_view_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode this module's generated `TimelineItem` table back into a
/// [`TimelineItemModel`]. Shared logic the `author_view` test decoder mirrors
/// against its own bindings.
pub fn timeline_item_from_fb(item: ti::TimelineItem<'_>) -> TimelineItemModel {
    TimelineItemModel {
        id: item.id().unwrap_or_default().to_string(),
        author_pubkey: item.author_pubkey().unwrap_or_default().to_string(),
        author_picture_url: item
            .has_author_picture_url()
            .then(|| item.author_picture_url().unwrap_or_default().to_string()),
        author_lnurl: item
            .has_author_lnurl()
            .then(|| item.author_lnurl().unwrap_or_default().to_string()),
        author_display_name: item
            .has_author_display_name()
            .then(|| item.author_display_name().unwrap_or_default().to_string()),
        kind: item.kind(),
        content: item.content().unwrap_or_default().to_string(),
        content_preview: item.content_preview().unwrap_or_default().to_string(),
        created_at: item.created_at(),
        relay_count: item.relay_count(),
        is_repost: item.is_repost(),
        nav_target_id: item.nav_target_id().unwrap_or_default().to_string(),
        repost_inner_content: item.repost_inner_content().unwrap_or_default().to_string(),
    }
}

/// Decode typed FlatBuffers bytes (as produced by [`encode_thread_view`]) back
/// into a [`ThreadViewModel`]. Returns an error string on any malformed input.
pub fn decode_thread_view(bytes: &[u8]) -> Result<ThreadViewModel, String> {
    if bytes.len() < 8 || !fb::thread_view_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KTVW file identifier".to_string());
    }
    let root = fb::root_as_thread_view_snapshot(bytes)
        .map_err(|e| format!("not a valid ThreadViewSnapshot buffer: {e}"))?;

    let mut items = Vec::new();
    if let Some(fb_items) = root.items() {
        items.reserve(fb_items.len());
        for item in fb_items.iter() {
            items.push(timeline_item_from_fb(item));
        }
    }

    Ok(ThreadViewModel {
        focused_event_id: root.focused_event_id().unwrap_or_default().to_string(),
        root_event_id: root.root_event_id().unwrap_or_default().to_string(),
        state: root.state().unwrap_or_default().to_string(),
        items,
        previous_count: root.previous_count(),
        next_count: root.next_count(),
        previous_count_label: root.previous_count_label().unwrap_or_default().to_string(),
        next_count_label: root.next_count_label().unwrap_or_default().to_string(),
    })
}

#[cfg(test)]
#[path = "thread_view_fb_tests.rs"]
mod tests;
