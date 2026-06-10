//! Typed FlatBuffers wire codec for the kernel-owned `"author_view"` projection
//! (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"author_view"`: the serialisation of `author_view()` (an
//! `AuthorViewPayload`) — **but only when an author view is open** (D5: the key
//! is OMITTED otherwise, never `null`). This module adds a **typed FlatBuffers**
//! encoding of the same shape, carried in the `typed_projections` sidecar
//! (ADR-0037) ALONGSIDE — never replacing — the generic `Value` projection, and
//! the typed entry is likewise pushed only when the view is open.
//!
//! [`AuthorViewModel`] is built from the same `author_view()` output the JSON
//! path serialises, in the same tick, so the two wire forms cannot diverge. It
//! reuses [`ProfileCardModel`](super::ProfileCardModel) and
//! [`TimelineItemModel`](super::TimelineItemModel) (the row shapes shared with
//! the `profile` / `thread_view` codecs), encoding them into THIS module's own
//! generated `ProfileCard` / `TimelineItem` tables.
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
#[path = "generated/author_view_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use super::{ProfileCardModel, TimelineItemModel};
use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub(crate) const AUTHOR_VIEW_SCHEMA_ID: &str = "author_view";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub(crate) const AUTHOR_VIEW_FILE_IDENTIFIER: &[u8; 4] = b"KAVW";
/// Wire schema version. Bump on any breaking change to `author_view.fbs`.
pub(crate) const AUTHOR_VIEW_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of `ProfileDispatchSpec` — the optional write-action
/// dispatch carried inside a [`ProfileActionModel`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ProfileDispatchSpecModel {
    pub(crate) namespace: String,
    pub(crate) body_json: String,
}

/// A field-for-field mirror of `ProfileAction` — `author_view`'s optional
/// `primary_action`. `dispatch` is `Some` for write verbs (follow / unfollow),
/// `None` for local-UI intents (edit sheet).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ProfileActionModel {
    pub(crate) kind: String,
    pub(crate) label: String,
    pub(crate) target_pubkey: String,
    pub(crate) icon_name: String,
    pub(crate) dispatch: Option<ProfileDispatchSpecModel>,
}

/// The `"author_view"` read model — a field-for-field mirror of
/// `AuthorViewPayload`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AuthorViewModel {
    pub(crate) pubkey: String,
    pub(crate) state: String,
    pub(crate) profile: ProfileCardModel,
    pub(crate) items: Vec<TimelineItemModel>,
    pub(crate) note_count: u64,
    pub(crate) note_count_display: String,
    pub(crate) primary_action: Option<ProfileActionModel>,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`ProfileCardModel`] into THIS module's generated `ProfileCard`
/// table. Mirrors `profile_fb::create_profile_card` against the `author_view`
/// bindings (a distinct generated type).
fn create_profile_card<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    card: &ProfileCardModel,
) -> WIPOffset<fb::ProfileCard<'a>> {
    let pubkey = fbb.create_string(&card.pubkey);
    let npub = fbb.create_string(&card.npub);
    let display_name = card
        .display_name
        .as_ref()
        .map(|value| fbb.create_string(value));
    let picture_url = card
        .picture_url
        .as_ref()
        .map(|value| fbb.create_string(value));
    let nip05 = fbb.create_string(&card.nip05);
    let about = fbb.create_string(&card.about);
    let lnurl = card.lnurl.as_ref().map(|value| fbb.create_string(value));
    fb::ProfileCard::create(
        fbb,
        &fb::ProfileCardArgs {
            pubkey: Some(pubkey),
            npub: Some(npub),
            has_display_name: card.display_name.is_some(),
            display_name,
            has_picture_url: card.picture_url.is_some(),
            picture_url,
            nip05: Some(nip05),
            about: Some(about),
            has_profile: card.has_profile,
            has_lnurl: card.lnurl.is_some(),
            lnurl,
        },
    )
}

/// Encode a `[TimelineItemModel]` slice into THIS module's generated
/// `TimelineItem` table offsets.
fn create_timeline_items<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    items: &[TimelineItemModel],
) -> WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<fb::TimelineItem<'a>>>> {
    let offsets: Vec<WIPOffset<fb::TimelineItem<'a>>> = items
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
            fb::TimelineItem::create(
                fbb,
                &fb::TimelineItemArgs {
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

/// Encode an optional [`ProfileActionModel`] into THIS module's generated
/// `ProfileAction` table.
fn create_primary_action<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    action: &ProfileActionModel,
) -> WIPOffset<fb::ProfileAction<'a>> {
    let kind = fbb.create_string(&action.kind);
    let label = fbb.create_string(&action.label);
    let target_pubkey = fbb.create_string(&action.target_pubkey);
    let icon_name = fbb.create_string(&action.icon_name);
    let dispatch = action.dispatch.as_ref().map(|spec| {
        let namespace = fbb.create_string(&spec.namespace);
        let body_json = fbb.create_string(&spec.body_json);
        fb::ProfileDispatchSpec::create(
            fbb,
            &fb::ProfileDispatchSpecArgs {
                namespace: Some(namespace),
                body_json: Some(body_json),
            },
        )
    });
    fb::ProfileAction::create(
        fbb,
        &fb::ProfileActionArgs {
            kind: Some(kind),
            label: Some(label),
            target_pubkey: Some(target_pubkey),
            icon_name: Some(icon_name),
            has_dispatch: action.dispatch.is_some(),
            dispatch,
        },
    )
}

/// Encode an [`AuthorViewModel`] to typed FlatBuffers bytes (with the `KAVW`
/// file identifier).
#[must_use]
pub(crate) fn encode_author_view(model: &AuthorViewModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let pubkey = fbb.create_string(&model.pubkey);
    let state = fbb.create_string(&model.state);
    let profile = create_profile_card(&mut fbb, &model.profile);
    let items = create_timeline_items(&mut fbb, &model.items);
    let note_count_display = fbb.create_string(&model.note_count_display);
    let primary_action = model
        .primary_action
        .as_ref()
        .map(|action| create_primary_action(&mut fbb, action));
    let root = fb::AuthorViewSnapshot::create(
        &mut fbb,
        &fb::AuthorViewSnapshotArgs {
            pubkey: Some(pubkey),
            state: Some(state),
            profile: Some(profile),
            items: Some(items),
            note_count: model.note_count,
            note_count_display: Some(note_count_display),
            has_primary_action: model.primary_action.is_some(),
            primary_action,
        },
    );
    fb::finish_author_view_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_author_view`]) back
/// into an [`AuthorViewModel`]. Returns an error string on any malformed input.
#[cfg(test)]
pub(crate) fn decode_author_view(bytes: &[u8]) -> Result<AuthorViewModel, String> {
    if bytes.len() < 8 || !fb::author_view_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KAVW file identifier".to_string());
    }
    let root = fb::root_as_author_view_snapshot(bytes)
        .map_err(|e| format!("not a valid AuthorViewSnapshot buffer: {e}"))?;

    let profile = root
        .profile()
        .map(profile_card_from_fb)
        .ok_or_else(|| "AuthorViewSnapshot missing profile".to_string())?;

    let mut items = Vec::new();
    if let Some(fb_items) = root.items() {
        items.reserve(fb_items.len());
        for item in fb_items.iter() {
            items.push(timeline_item_from_fb(item));
        }
    }

    // `has_primary_action` gates presence (mirroring the JSON `null`), and the
    // generated `primary_action()` is itself `Option` — `and_then` honours both
    // without any `.expect()` (D6: no panics at the decode boundary).
    let primary_action = root
        .has_primary_action()
        .then(|| root.primary_action())
        .flatten()
        .map(|action| {
            let dispatch = action
                .has_dispatch()
                .then(|| action.dispatch())
                .flatten()
                .map(|spec| ProfileDispatchSpecModel {
                    namespace: spec.namespace().unwrap_or_default().to_string(),
                    body_json: spec.body_json().unwrap_or_default().to_string(),
                });
            ProfileActionModel {
                kind: action.kind().unwrap_or_default().to_string(),
                label: action.label().unwrap_or_default().to_string(),
                target_pubkey: action.target_pubkey().unwrap_or_default().to_string(),
                icon_name: action.icon_name().unwrap_or_default().to_string(),
                dispatch,
            }
        });

    Ok(AuthorViewModel {
        pubkey: root.pubkey().unwrap_or_default().to_string(),
        state: root.state().unwrap_or_default().to_string(),
        profile,
        items,
        note_count: root.note_count(),
        note_count_display: root.note_count_display().unwrap_or_default().to_string(),
        primary_action,
    })
}

/// Decode THIS module's generated `ProfileCard` table into a
/// [`ProfileCardModel`] (mirrors `profile_fb::profile_card_from_fb` against the
/// author_view bindings).
#[cfg(test)]
fn profile_card_from_fb(card: fb::ProfileCard<'_>) -> ProfileCardModel {
    ProfileCardModel {
        pubkey: card.pubkey().unwrap_or_default().to_string(),
        npub: card.npub().unwrap_or_default().to_string(),
        display_name: card
            .has_display_name()
            .then(|| card.display_name().unwrap_or_default().to_string()),
        picture_url: card
            .has_picture_url()
            .then(|| card.picture_url().unwrap_or_default().to_string()),
        nip05: card.nip05().unwrap_or_default().to_string(),
        about: card.about().unwrap_or_default().to_string(),
        has_profile: card.has_profile(),
        lnurl: card
            .has_lnurl()
            .then(|| card.lnurl().unwrap_or_default().to_string()),
    }
}

/// Decode THIS module's generated `TimelineItem` table into a
/// [`TimelineItemModel`] (mirrors `thread_view_fb::timeline_item_from_fb`).
#[cfg(test)]
fn timeline_item_from_fb(item: fb::TimelineItem<'_>) -> TimelineItemModel {
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

#[cfg(test)]
#[path = "author_view_fb_tests.rs"]
mod tests;
