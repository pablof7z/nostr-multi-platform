//! Reference resolution UniFFI surface (#2125).
//!
//! `nmp-uniffi` is the sole native binding surface for ADR-0070 Lane D
//! reference resolution (M14 complete; the legacy `nmp-ffi` C-ABI crate has
//! been deleted). Each sub-module adds a `#[uniffi::export] impl NmpApp`
//! block exposing typed methods.
//!
//! ## Module layout
//!
//! | Module    | UniFFI methods                                                    |
//! |-----------|---------------------------------------------------------------------|
//! | `ref_`    | `resolve_ref`, `resolve_ref_with_metadata`, `release_ref`        |
//! | `profile` | `resolve_profile_ref`, `resolve_profile_card_live`, `release_profile_ref` |
//! | `embed`   | `resolve_event_embed`, `resolve_event_embed_live`, `resolve_event_embed_with_metadata`, `resolve_event_embed_live_with_metadata`, `release_event_ref` |
//!
//! ## Reactive-lifecycle design note
//!
//! The "live" resolve variants register a per-key, per-consumer reactive
//! subscription. The teardown (`release_*`) uses the same `consumer_id` string
//! that was passed to the resolve call. There is NO returned handle object and
//! NO UniFFI callback interface for these resolves: `consumer_id` IS the
//! subscription handle. This is identical to the C-ABI model; the direct
//! mapping is sound and idiomatic in UniFFI.
//!
//! Release is idempotent (D6): the kernel's refcount map is a no-op on
//! unknown (namespace, key, consumer_id) triples.

pub mod embed;
pub mod profile;
pub mod ref_;

// ── Shared UniFFI types ───────────────────────────────────────────────────────

/// Reference namespace discriminant.
///
/// `Profile` — key is a 64-hex-char lowercase pubkey.
/// `Event` — key is a hex event-id, `"kind:pubkey:d"` coordinate, or
/// `"i:<external-id>"` NIP-73 external reference.
#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum RefNamespace {
    Profile,
    Event,
}

/// Shape discriminant for the `Profile` namespace.
///
/// `Ref` — minimal feed-avatar shape `{pubkey, display_name, picture_url}`.
/// `Card` — full `ProfileCard`; used for open profile screens.
#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum ProfileShape {
    Ref,
    Card,
}

/// Shape discriminant for the `Event` namespace.
///
/// `Embed` — the render-an-embed-card subset.
/// `Raw` — the full raw event.
#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum EventShape {
    Embed,
    Raw,
}

/// Combined namespace+shape discriminant.
///
/// UniFFI uses struct-style variant fields for associated data.
/// Variant `Profile` is only valid with `RefNamespace::Profile`;
/// variant `Event` is only valid with `RefNamespace::Event`.
/// The kernel's `resolve_ref` front door validates the pairing and
/// fails closed (D6) on mismatch.
#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum RefShape {
    Profile { shape: ProfileShape },
    Event { shape: EventShape },
}

/// Freshness policy for a reference resolution.
///
/// `CacheOk` — serve from store + one-shot fetch on miss; no live sub.
///   Use for feed-row avatars and background embed claims.
/// `Live` — keep a tailing subscription open while the consumer holds
///   the key. Use for open profile screens and live-updating embeds.
///   `Live` wins on dedup when multiple consumers resolve the same key.
#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum RefLiveness {
    CacheOk,
    Live,
}

/// Optional caller-supplied relay + author metadata for a raw-key resolve.
///
/// Used by app-owned URI adapters that decode `nostr:` / NIP-19 values
/// before crossing the FFI boundary. The key is always raw (never a URI).
///
/// `hints` — relay URLs decoded from NIP-19/NIP-21 TLVs.
/// `event_author` — optional hex-pubkey author decoded from a nevent
///   author TLV. Ignored for profile refs and superseded by
///   coordinate-derived authors for naddr keys.
#[derive(uniffi::Record, Debug, Clone)]
pub struct ResolveMetadata {
    pub hints: Vec<String>,
    pub event_author: Option<String>,
}

// ── Conversion helpers (UniFFI types → nmp-core types) ───────────────────────

pub(super) fn to_core_namespace(ns: &RefNamespace) -> nmp_core::RefNamespace {
    match ns {
        RefNamespace::Profile => nmp_core::RefNamespace::Profile,
        RefNamespace::Event => nmp_core::RefNamespace::Event,
    }
}

pub(super) fn to_core_shape(shape: &RefShape) -> nmp_core::RefShape {
    match shape {
        RefShape::Profile { shape: s } => nmp_core::RefShape::Profile(match s {
            ProfileShape::Ref => nmp_core::ProfileShape::Ref,
            ProfileShape::Card => nmp_core::ProfileShape::Card,
        }),
        RefShape::Event { shape: s } => nmp_core::RefShape::Event(match s {
            EventShape::Embed => nmp_core::EventShape::Embed,
            EventShape::Raw => nmp_core::EventShape::Raw,
        }),
    }
}

pub(super) fn to_core_liveness(liveness: &RefLiveness) -> nmp_core::RefLiveness {
    match liveness {
        RefLiveness::CacheOk => nmp_core::RefLiveness::CacheOk,
        RefLiveness::Live => nmp_core::RefLiveness::Live,
    }
}

pub(super) fn to_core_metadata(metadata: ResolveMetadata) -> nmp_core::RefResolveMetadata {
    nmp_core::RefResolveMetadata {
        hints: metadata.hints,
        event_author: metadata.event_author,
    }
}
