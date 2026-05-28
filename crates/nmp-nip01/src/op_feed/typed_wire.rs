//! Typed FlatBuffers wire encoding for the OP-centric home feed
//! (`nmp_feed::RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution>`).
//!
//! This is the `nmp-nip01`-owned typed projection of the V-80 OP-centric home
//! feed: one `nmp.nip01.OpFeedSnapshot` buffer (file identifier `NOFS`)
//! carrying the visible-window root cards, each with its raw NIP-10 reply
//! attribution list, plus the embedded typed `nmp-feed` window.
//!
//! Relates to ADR-0038 (this schema), ADR-0037 (typed FlatBuffers runtime
//! projections — the sidecar transport + descriptor), and ADR-0032 (raw-data
//! projection doctrine).
//!
//! ## Reuse, not duplication (ADR-0038 Commitment 2)
//!
//! * `RootCard.card` is built by the **identical** per-card encoder NFTS uses
//!   (`crate::typed_wire::encode::encode_card`), so the embedded typed NFCT
//!   content tree and `content_render` bytes are byte-for-byte the same as the
//!   timeline path — no re-encode.
//! * The paging envelope (`page` / `metrics`) travels as the typed `nmp-feed`
//!   `FeedWindow` buffer (`NFWM`) embedded as opaque `feed_window_bytes`, via
//!   `nmp_feed::encode_feed_window`. `nmp-nip01` never re-declares cursor tables.
//!
//! ## Raw data only (ADR-0032)
//!
//! Every field is the raw protocol value: hex pubkeys / event ids, Unix-second
//! `created_at`, verbatim kind:0 display mirrors with `has_*` absence flags. No
//! `display::` forwarder runs on the encode path.
//!
//! ## D5 bound
//!
//! `RootCard.attribution` is bounded at encode time by
//! [`MAX_ATTRIBUTION_PER_ROOT`](nmp_feed::MAX_ATTRIBUTION_PER_ROOT): the engine
//! already caps the per-root attribution sub-map at ingest, but the encoder
//! re-bounds defensively so the wire vector can never exceed the cap.
//!
//! ## Regenerating the bindings
//!
//! The checked-in bindings in `wire/generated/op_feed_generated.rs` are produced
//! by `flatc` from `schema/op_feed.fbs`. Regenerate only with the workspace
//! FlatBuffers pin (`25.12.19`), enforced by
//! `ci/check-flatbuffers-version-pins.sh`. Because the schema uses an `include`,
//! generate WITHOUT `--gen-all`:
//!
//! ```sh
//! flatc --rust -o crates/nmp-nip01/src/wire/generated \
//!       crates/nmp-nip01/schema/op_feed.fbs
//! ```

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use nmp_feed::{FeedWindowWire, RootCard, RootFeedSnapshot, MAX_ATTRIBUTION_PER_ROOT};

use super::attribution::Nip10ReplyAttribution;
use crate::profile_display::AuthorDisplay;
use crate::timeline_projection::TimelineEventCard;

// The generated bindings live in two crate-root modules (see `lib.rs`):
//
//   * `fb` — the OP-feed-native tables (`OpFeedSnapshot`, `RootCard`,
//     `ReplyAttribution`) + the root buffer helpers, under
//     `op_feed_generated::nmp::nip_01`.
//   * `tl` — the *included* timeline tables (`TimelineEventCard`,
//     `AuthorDisplay`), referenced through the flat-re-exported timeline
//     wrapper. `op_feed_generated` only privately re-imports these via its
//     `use crate::timeline_snapshot_generated::*` glob, so the shared types
//     must be named through `tl`, never `fb` (the latter would hit a private
//     re-import).
#[allow(clippy::wildcard_imports)]
use crate::op_feed_generated::nmp::nip_01 as fb;
use crate::timeline_snapshot_generated as tl;

/// The concrete OP-feed snapshot shape this codec encodes / decodes.
///
/// `RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution>` — exactly what
/// the NIP-10 [`OpFeedEngine`](super::OpFeedEngine) produces. Chirp's
/// `ChirpTimelineSnapshot` is a type alias of this same instantiation, so no
/// wrapper conversion is needed at the registration site (ADR-0038 §Encoding
/// notes).
pub type OpFeedSnapshot = RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution>;

/// Stable projection schema identity this wire shape projects into.
pub const OP_FEED_SCHEMA_ID: &str = "nmp.nip01.opfeed";

/// FlatBuffers file identifier for an `OpFeedSnapshot` root buffer.
pub const OP_FEED_FILE_IDENTIFIER: &[u8; 4] = b"NOFS";

/// Schema version of the typed OP-feed payload. Bump on any breaking field
/// change. Mirrors `OpFeedSnapshot.schema_version` in the `.fbs`.
pub const OP_FEED_SCHEMA_VERSION: u32 = 1;

// ===========================================================================
// Encode
// ===========================================================================

/// Encode a [`RootFeedSnapshot`] as one typed FlatBuffers `OpFeedSnapshot`
/// buffer with the `NOFS` file identifier.
#[must_use]
pub fn encode_op_feed_snapshot(snapshot: &OpFeedSnapshot) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();

    let cards: Vec<WIPOffset<fb::RootCard<'_>>> = snapshot
        .cards
        .iter()
        .map(|root| encode_root_card(&mut builder, root))
        .collect();
    let cards = builder.create_vector(&cards);

    let has_page = snapshot.page.is_some();
    let has_metrics = snapshot.metrics.is_some();
    let feed_window_bytes = encode_feed_window_bytes(snapshot)
        .as_ref()
        .map(|bytes| builder.create_vector(bytes));

    let root = fb::OpFeedSnapshot::create(
        &mut builder,
        &fb::OpFeedSnapshotArgs {
            schema_version: OP_FEED_SCHEMA_VERSION,
            cards: Some(cards),
            feed_window_bytes,
            has_page,
            has_metrics,
        },
    );
    fb::finish_op_feed_snapshot_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

fn encode_root_card<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    root: &RootCard<TimelineEventCard, Nip10ReplyAttribution>,
) -> WIPOffset<fb::RootCard<'bldr>> {
    // Reuse the identical NFTS per-card encoder so the embedded NFCT content
    // tree / content_render bytes match the timeline path exactly.
    let card = crate::typed_wire::encode::encode_card(builder, &root.card);

    // D5: re-bound the attribution vector defensively (the engine caps at
    // ingest; this guarantees the wire vector never exceeds the cap).
    let attribution: Vec<WIPOffset<fb::ReplyAttribution<'_>>> = root
        .attribution
        .iter()
        .take(MAX_ATTRIBUTION_PER_ROOT)
        .map(|attr| encode_reply_attribution(builder, attr))
        .collect();
    let attribution = builder.create_vector(&attribution);

    fb::RootCard::create(
        builder,
        &fb::RootCardArgs {
            card: Some(card),
            attribution: Some(attribution),
        },
    )
}

fn encode_reply_attribution<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    attr: &Nip10ReplyAttribution,
) -> WIPOffset<fb::ReplyAttribution<'bldr>> {
    let author_display = encode_author_display(builder, &attr.author_display);
    let author_pubkey = builder.create_string(&attr.author_pubkey);
    let reply_event_id = builder.create_string(&attr.reply_event_id);
    let author_display_name = attr
        .author_display_name
        .as_ref()
        .map(|s| builder.create_string(s));
    let author_picture_url = attr
        .author_picture_url
        .as_ref()
        .map(|s| builder.create_string(s));
    fb::ReplyAttribution::create(
        builder,
        &fb::ReplyAttributionArgs {
            author_pubkey: Some(author_pubkey),
            author_display: Some(author_display),
            has_author_display_name: attr.author_display_name.is_some(),
            author_display_name,
            has_author_picture_url: attr.author_picture_url.is_some(),
            author_picture_url,
            reply_event_id: Some(reply_event_id),
            reply_created_at: attr.reply_created_at,
        },
    )
}

fn encode_author_display<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    display: &AuthorDisplay,
) -> WIPOffset<tl::AuthorDisplay<'bldr>> {
    let name = display.name.as_ref().map(|s| builder.create_string(s));
    let npub = display.npub.as_ref().map(|s| builder.create_string(s));
    let picture_url = display
        .picture_url
        .as_ref()
        .map(|s| builder.create_string(s));
    tl::AuthorDisplay::create(
        builder,
        &tl::AuthorDisplayArgs {
            has_name: display.name.is_some(),
            name,
            has_npub: display.npub.is_some(),
            npub,
            has_picture_url: display.picture_url.is_some(),
            picture_url,
        },
    )
}

/// Encode the embedded typed `nmp-feed` `FeedWindow` (`NFWM`) sub-buffer, or
/// `None` when both `page` and `metrics` are absent (the empty / diagnostics
/// case — matches the NFTS empty-window precedent).
fn encode_feed_window_bytes(snapshot: &OpFeedSnapshot) -> Option<Vec<u8>> {
    if snapshot.page.is_none() && snapshot.metrics.is_none() {
        return None;
    }
    Some(nmp_feed::encode_feed_window(&FeedWindowWire {
        page: snapshot.page.clone(),
        metrics: snapshot.metrics.clone(),
    }))
}

// ===========================================================================
// Decode
// ===========================================================================

/// Decode a typed FlatBuffers `OpFeedSnapshot` buffer back into the owned
/// [`RootFeedSnapshot`]. Returns a human-readable error string on any
/// malformed-buffer or missing-required-field condition.
pub fn decode_op_feed_snapshot(bytes: &[u8]) -> Result<OpFeedSnapshot, String> {
    if bytes.len() < 8 || !fb::op_feed_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NOFS file identifier".to_string());
    }
    let snapshot = fb::root_as_op_feed_snapshot(bytes).map_err(|err| format!("{err:?}"))?;

    let mut cards = Vec::new();
    if let Some(fb_cards) = snapshot.cards() {
        cards.reserve(fb_cards.len());
        for index in 0..fb_cards.len() {
            cards.push(decode_root_card(fb_cards.get(index))?);
        }
    }

    // The `has_page` / `has_metrics` flags duplicate presence info inside the
    // embedded NFWM window; the window is the authoritative source on decode
    // (it carries the actual `page` / `metrics` values), so the flags are
    // advisory and not consulted here.
    let window = decode_feed_window_bytes(snapshot.feed_window_bytes())?;

    Ok(RootFeedSnapshot {
        cards,
        page: window.page,
        metrics: window.metrics,
    })
}

fn decode_root_card(
    root: fb::RootCard<'_>,
) -> Result<RootCard<TimelineEventCard, Nip10ReplyAttribution>, String> {
    let card = crate::typed_wire::decode::decode_card(
        root.card().ok_or("RootCard missing card")?,
    )?;
    let mut attribution = Vec::new();
    if let Some(attrs) = root.attribution() {
        attribution.reserve(attrs.len());
        for index in 0..attrs.len() {
            attribution.push(decode_reply_attribution(attrs.get(index))?);
        }
    }
    Ok(RootCard { card, attribution })
}

fn decode_reply_attribution(
    attr: fb::ReplyAttribution<'_>,
) -> Result<Nip10ReplyAttribution, String> {
    let author_display = decode_author_display(
        attr.author_display()
            .ok_or("ReplyAttribution missing author_display")?,
    );
    Ok(Nip10ReplyAttribution {
        author_pubkey: attr
            .author_pubkey()
            .ok_or("ReplyAttribution missing author_pubkey")?
            .to_string(),
        author_display,
        author_display_name: optional_string(
            attr.has_author_display_name(),
            attr.author_display_name(),
        ),
        author_picture_url: optional_string(
            attr.has_author_picture_url(),
            attr.author_picture_url(),
        ),
        reply_event_id: attr
            .reply_event_id()
            .ok_or("ReplyAttribution missing reply_event_id")?
            .to_string(),
        reply_created_at: attr.reply_created_at(),
    })
}

fn decode_author_display(display: tl::AuthorDisplay<'_>) -> AuthorDisplay {
    AuthorDisplay {
        name: optional_string(display.has_name(), display.name()),
        npub: optional_string(display.has_npub(), display.npub()),
        picture_url: optional_string(display.has_picture_url(), display.picture_url()),
    }
}

/// Decode the embedded typed `nmp-feed` `FeedWindow` buffer. Absent or empty
/// bytes decode to the default empty window (page / metrics both `None`).
fn decode_feed_window_bytes(
    bytes: Option<flatbuffers::Vector<'_, u8>>,
) -> Result<FeedWindowWire, String> {
    match bytes {
        Some(v) if !v.bytes().is_empty() => {
            nmp_feed::decode_feed_window(v.bytes()).map_err(|err| format!("feed_window: {err}"))
        }
        _ => Ok(FeedWindowWire::default()),
    }
}

/// Reconstruct an `Option<String>` from a `has_*` flag + the wire string,
/// distinguishing absent (`None`) from present-empty (`Some("")`).
fn optional_string(present: bool, value: Option<&str>) -> Option<String> {
    if present {
        Some(value.unwrap_or_default().to_string())
    } else {
        None
    }
}

#[cfg(test)]
#[path = "typed_wire_tests.rs"]
mod tests;
