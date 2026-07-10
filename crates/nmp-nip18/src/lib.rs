//! `nmp-nip18` — NIP-18 repost decoding and read-surfacing primitives.
//!
//! This crate owns generic repost wire interpretation. It does not render UI,
//! choose relay policy, or depend on any app crate.

use nmp_core::substrate::KernelEvent;
use nmp_nip09::AddressCoordinate;
use serde::Deserialize;

mod action;
mod lane_mapping;
mod primary_kind;
mod repost_projection;
mod repost_target;
mod wire;

pub use action::{
    build_repost_event, QuoteRepostAction, QuoteRepostModule, RepostAction, RepostModule,
};
pub use lane_mapping::{
    nip18_target_mapping, nip18_target_render_only_mapping, NIP18_TARGET_MAPPING_ID,
};
pub use primary_kind::{
    acquisition_kinds_for_primary, try_acquisition_kinds_for_primary, validate_primary_kinds,
    PrimaryKindError,
};
pub use repost_projection::{
    repost_activity_interest_shape, RepostActivity, RepostActivityProjection, RepostObservation,
    RepostTarget,
};
pub use repost_target::resolve_repost_target;

/// NIP-18 repost event kind for kind:1 short-text notes.
pub const KIND_REPOST: u32 = 6;

/// NIP-18 generic repost event kind for non-kind:1 targets.
pub const KIND_GENERIC_REPOST: u32 = 16;

/// NIP-09 deletion event kind. Not owned here — `nmp-nip09` is the exclusive
/// owner of kind:5 construction/parsing (ADR-0074) — but every repost
/// acquisition shape and delete-fold call site in this crate needs the kind
/// literal to subscribe to and recognise deletes of repost wrappers.
pub const KIND_DELETE: u32 = 5;

/// Return whether `kind` is a NIP-18 repost wrapper kind.
#[must_use]
pub const fn is_repost_kind(kind: u32) -> bool {
    kind == KIND_REPOST || kind == KIND_GENERIC_REPOST
}

/// Decoded inner event embedded in a repost `content` field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedEvent {
    pub id: String,
    pub author: String,
    pub kind: u32,
    pub created_at: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

/// Decoded NIP-18 repost record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepostRecord {
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub target_event_id: Option<String>,
    pub target_kind: Option<u32>,
    /// Address coordinate of the target, when the wrapper carries an `a` tag (a
    /// generic repost of a replaceable/addressable event) or embeds an
    /// addressable event. This is the canonical row identity for addressable
    /// targets — present means the coordinate is *proven*, never guessed from an
    /// event id. See [`nmp_nip09::AddressCoordinate`].
    pub target_address: Option<AddressCoordinate>,
    pub embedded_event: Option<EmbeddedEvent>,
    /// Author of the reposted event, when provable from data the repost
    /// wrapper itself carries: an explicit `p` tag (NIP-18 §"reposts SHOULD
    /// include a `p` tag with the pubkey of the author of the reposted
    /// event"), or the embedded event's own `pubkey`. `None` means the
    /// wrapper is tag-only and non-compliant (no `p` tag) — the author is
    /// simply unknown until the target itself is delivered; this is never
    /// resolved via a by-id lookup (#3124).
    pub target_author_pubkey: Option<String>,
}

/// Decode a [`KernelEvent`] as a NIP-18 repost.
///
/// Returns `None` for every non-repost event. A repost with only an `e` tag and
/// no embedded event is still a repost record; consumers can render a
/// placeholder while the target is unresolved.
#[must_use]
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<RepostRecord> {
    if !is_repost_kind(event.kind) {
        return None;
    }

    let embedded_event = parse_embedded_event(&event.content);
    let target_event_id = first_event_tag(&event.tags)
        .or_else(|| embedded_event.as_ref().map(|inner| inner.id.clone()));
    let target_kind =
        first_kind_tag(&event.tags).or_else(|| embedded_event.as_ref().map(|inner| inner.kind));
    // Prefer the explicit `a` tag (proven coordinate). Otherwise derive the
    // coordinate from an embedded addressable event — its (kind, pubkey, d) is
    // fully known. Never derive a coordinate from a bare `e`/`k` pair: an event
    // id cannot prove a coordinate, so such a target stays address-unresolved.
    let target_address = first_address_tag(&event.tags).or_else(|| {
        embedded_event.as_ref().and_then(|inner| {
            AddressCoordinate::from_event(&KernelEvent {
                id: inner.id.clone(),
                author: inner.author.clone(),
                kind: inner.kind,
                created_at: inner.created_at,
                tags: inner.tags.clone(),
                content: inner.content.clone(),
                relay_provenance: Vec::new(),
            })
        })
    });

    let target_author_pubkey = first_pubkey_tag(&event.tags)
        .or_else(|| embedded_event.as_ref().map(|inner| inner.author.clone()));

    Some(RepostRecord {
        event_id: event.id.clone(),
        author: event.author.clone(),
        created_at: event.created_at,
        target_event_id,
        target_kind,
        target_address,
        embedded_event,
        target_author_pubkey,
    })
}

#[derive(Deserialize)]
struct EmbeddedEventWire {
    id: String,
    pubkey: String,
    kind: u32,
    created_at: u64,
    #[serde(default)]
    tags: Vec<Vec<String>>,
    content: String,
}

fn parse_embedded_event(raw: &str) -> Option<EmbeddedEvent> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let wire: EmbeddedEventWire = serde_json::from_str(trimmed).ok()?;
    Some(EmbeddedEvent {
        id: wire.id,
        author: wire.pubkey,
        kind: wire.kind,
        created_at: wire.created_at,
        tags: wire.tags,
        content: wire.content,
    })
}

fn first_event_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "e") {
            tag.get(1).filter(|id| !id.is_empty()).cloned()
        } else {
            None
        }
    })
}

fn first_pubkey_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "p") {
            tag.get(1).filter(|pubkey| !pubkey.is_empty()).cloned()
        } else {
            None
        }
    })
}

fn first_kind_tag(tags: &[Vec<String>]) -> Option<u32> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "k") {
            tag.get(1).and_then(|raw| raw.parse::<u32>().ok())
        } else {
            None
        }
    })
}

fn first_address_tag(tags: &[Vec<String>]) -> Option<AddressCoordinate> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "a") {
            tag.get(1).and_then(|raw| AddressCoordinate::parse(raw))
        } else {
            None
        }
    })
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

#[derive(Clone, Debug, Default)]
pub struct Config {}

#[derive(Clone, Debug, Default)]
pub struct Handles {}

pub fn register(
    app: &mut impl nmp_core::substrate::ActionRegistrar,
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    action::register_actions(app);
    Ok(Handles {})
}
