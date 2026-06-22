//! ADR-0063 (#1671 integration glue, codex "Artifact 1") — `impl RefRowRevSource
//! for Kernel`.
//!
//! This is the seam that wires Lane B's resolver state (per-key revs +
//! demanded-shape maps + refcounts) into Lane A's row-delta carrier
//! ([`crate::refs::RefRowDeltaTracker`]). The tracker consumes EXACTLY this
//! trait — it never reimplements resolution — so the kernel is the production
//! `RefRowRevSource` that replaces the test-only `MapRowRevSource` stub.
//!
//! ## The three methods
//!
//! - [`Kernel::ref_row_rev`](RefRowRevSource::ref_row_rev) — maps the wire
//!   namespace string to [`RefNamespace`] and returns Lane B's per-KEY rev
//!   (monotonic through release; 0 for never-seen).
//! - [`Kernel::ref_row_keys`](RefRowRevSource::ref_row_keys) — enumerates the
//!   **LIVE** key set from the demanded-shape maps (`ref_profile_shapes` /
//!   `ref_event_shapes`), **NOT** from the per-key rev maps. CRITICAL: the rev
//!   maps retain a key until its final-`Clear` teardown (and across a host-late
//!   tick), so enumerating them would resurrect released rows on a baseline. A
//!   key is live iff a consumer currently demands a shape for it.
//! - [`Kernel::ref_row_payload`](RefRowRevSource::ref_row_payload) — builds the
//!   typed row for the WIDEST demanded shape, reading the SAME kernel accessors
//!   the legacy `claimed_profiles` / `claimed_events` projections read
//!   (`profile_card_for`, `lookup_for_primary_id` + the content-parser seam).
//!   `None` when the entity is not yet resolvable (absence ⇒ Unchanged on the
//!   carrier, never Cleared — invariant #1).
//!
//! ## Wire namespace strings
//!
//! Lane A keys the two batches by the bare namespace token `"profile"` /
//! `"event"` (see `RefRowDeltaBatch::namespace`). [`namespace_from_wire`] maps
//! those tokens to [`RefNamespace`]; an unknown token fails closed (no rows).

use super::refs::{EventShape, ProfileShape, RefNamespace};
use super::typed_projections::{
    encode_claimed_events, encode_profile, ClaimedEventRow, ClaimedEventsModel, ProfileCardModel,
};
use super::types::ClaimedEventDto;
use super::Kernel;
use crate::refs::RefRowRevSource;

/// The bare wire namespace token for the profile resolver (Lane A batch key).
pub(crate) const REF_NS_PROFILE: &str = "profile";
/// The bare wire namespace token for the event resolver (Lane A batch key).
pub(crate) const REF_NS_EVENT: &str = "event";

/// Map a Lane A wire namespace token to the typed [`RefNamespace`]. Unknown
/// tokens return `None` (fail closed — the carrier then emits no rows for them).
fn namespace_from_wire(namespace: &str) -> Option<RefNamespace> {
    match namespace {
        REF_NS_PROFILE => Some(RefNamespace::Profile),
        REF_NS_EVENT => Some(RefNamespace::Event),
        _ => None,
    }
}

impl Kernel {
    /// Build the typed `refs.profile` row payload for `key` at the demanded
    /// profile shape. Reads the SAME `profile_card_for` accessor the `refs.profile`
    /// projection reads, then narrows the encoded card to the
    /// widest shape any live consumer demanded (D5). `None` once no consumer
    /// holds the key (it is not live) — the caller treats that as not-resolvable.
    fn ref_profile_row_payload(&self, key: &str) -> Option<Vec<u8>> {
        let shape = self.ref_demanded_profile_shape(key)?;
        // Not-resolvable-yet ⇒ `None` (absence on the carrier ⇒ Unchanged, never
        // Cleared — ADR-0063 invariant #1). A claimed pubkey with no cached kind:0
        // is live but unresolved; `profile_card_for` would synthesize a
        // placeholder card, so gate on the real cache presence here.
        self.profile_for_pubkey(key)?;
        let card = self.profile_card_for(key, "");
        let mut model = ProfileCardModel {
            pubkey: card.pubkey,
            // ADR-0032 / V-115: npub removed from ProfileCard; shells encode
            // bech32 host-side. Empty here, matching the other profile codecs.
            npub: String::new(),
            display_name: card.display_name,
            name: card.name,
            raw_display_name: card.raw_display_name,
            display_name_camel: card.display_name_camel,
            picture_url: card.picture_url,
            banner: card.banner,
            website: card.website,
            nip05: card.nip05,
            about: card.about,
            lud16: card.lud16,
            lud06: card.lud06,
            lnurl: card.lnurl,
        };
        // ADR-0063 D5 shape narrowing: `Ref` is the feed-avatar subset
        // `{pubkey, display_name, picture_url}`; `Card` carries every field.
        // Narrowing drops the wide-only fields so a feed-avatar consumer's row
        // does not ship the full profile-screen payload.
        if matches!(shape, ProfileShape::Ref) {
            model.name = None;
            model.raw_display_name = None;
            model.display_name_camel = None;
            model.banner = None;
            model.website = None;
            model.nip05 = String::new();
            model.about = String::new();
            model.lud16 = None;
            model.lud06 = None;
            model.lnurl = None;
        }
        Some(encode_profile(&model))
    }

    /// Build the typed `refs.event` row payload for `key` at the demanded event
    /// shape. Reads the SAME `lookup_for_primary_id` + content-parser seam the
    /// legacy `claimed_events` projection reads. `None` when the key is not live
    /// OR the event is not yet in the store (not-resolvable ⇒ Unchanged, never
    /// Cleared). The single-entry [`ClaimedEventsModel`] reuses the existing,
    /// round-tripping `claimed_events` codec so the carrier payload decodes with
    /// the production decoder.
    fn ref_event_row_payload(&self, key: &str) -> Option<Vec<u8>> {
        let shape = self.ref_demanded_event_shape(key)?;
        let stored = self.lookup_for_primary_id(key)?;
        // `Raw` carries the parsed NFCT content tree (the full embed render
        // surface); `Embed` is the lighter render-an-embed-card subset, so it
        // omits the heavier content-tree bytes. Both carry the raw event fields
        // (id / author / kind / created_at / tags / content) — the event twin of
        // the profile `Ref`/`Card` narrowing.
        let content_tree_bytes = if matches!(shape, EventShape::Raw) {
            self.content_parser
                .parse_to_nfct_bytes(&stored.content, &stored.tags, stored.kind)
        } else {
            Vec::new()
        };
        let dto: ClaimedEventDto =
            ClaimedEventDto::from_stored(key.to_string(), &stored).with_content_tree(content_tree_bytes);
        let model = ClaimedEventsModel {
            entries: vec![(
                key.to_string(),
                ClaimedEventRow {
                    primary_id: dto.primary_id.clone(),
                    id: dto.id.clone(),
                    author_pubkey: dto.author_pubkey.clone(),
                    author_display_name: dto.author_display_name.clone(),
                    author_picture_url: dto.author_picture_url.clone(),
                    kind: dto.kind,
                    created_at: dto.created_at,
                    tags: dto.tags.clone(),
                    content: dto.content.clone(),
                    content_tree_bytes: dto.content_tree_bytes.clone(),
                },
            )],
        };
        Some(encode_claimed_events(&model))
    }
}

impl RefRowRevSource for Kernel {
    fn ref_row_rev(&self, namespace: &str, key: &str) -> u64 {
        match namespace_from_wire(namespace) {
            Some(ns) => self.ref_row_rev(ns, key),
            None => 0,
        }
    }

    fn ref_row_keys(&self, namespace: &str) -> Vec<String> {
        // CRITICAL (ADR-0063): enumerate the LIVE key set from the demanded-shape
        // maps, NOT the per-key rev maps. The rev maps retain a key through its
        // final-`Clear` teardown; enumerating them would resurrect a released row
        // on the next baseline. A key is live iff a consumer currently demands a
        // shape for it (`ref_profile_shapes` / `ref_event_shapes`).
        match namespace_from_wire(namespace) {
            Some(RefNamespace::Profile) => self.ref_profile_shapes.keys().cloned().collect(),
            Some(RefNamespace::Event) => self.ref_event_shapes.keys().cloned().collect(),
            None => Vec::new(),
        }
    }

    fn ref_row_payload(&self, namespace: &str, key: &str) -> Option<Vec<u8>> {
        match namespace_from_wire(namespace) {
            Some(RefNamespace::Profile) => self.ref_profile_row_payload(key),
            Some(RefNamespace::Event) => self.ref_event_row_payload(key),
            None => None,
        }
    }
}
