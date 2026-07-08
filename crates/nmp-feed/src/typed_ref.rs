//! [`TypedRef`] — a delivery-tagged, closed typed pointer a [`crate::FeedRow`]
//! declares.
//!
//! A row's `refs` vector replaces the single-purpose `RenderTarget` pointer
//! (#3082's provisional shape). Every ref names a target (an event id or a
//! replaceable address) AND a [`DeliveryMode`] saying how the engine treats it:
//!
//! * [`DeliveryMode::RenderOnly`] — declared into the `refs.event` / embed
//!   render channel only (ADR-0070 D7, [`crate::CardAuthors::rendered_target_refs`]).
//!   The feed sink never receives this target as a delivered row; a shell
//!   resolves it lazily (a quote-card preview). This is the existing repost
//!   render-target behavior, carried forward unchanged in shape.
//! * [`DeliveryMode::Delivered`] — the target's key is folded into the feed
//!   session's OWN `live_shapes` + admission, so the target re-enters via
//!   `on_kernel_event` as a REAL delivered [`nmp_core::substrate::KernelEvent`],
//!   carrying its true `created_at` and producing its own provenance
//!   contribution. This is the composite-lane hydration primitive
//!   (`crates/nmp-feed-session/src/delivered_ref.rs` generalizes the mechanism
//!   `pointer_target_hydration.rs` pioneered for reaction/comment targets).
//!
//! AT MOST ONE `Delivered` ref per row (the row has exactly one thing it is
//! "about" that must be hydrated to its real form). Callers that union refs
//! across sources (the composite merge) must preserve this invariant — see
//! [`merge_refs`].
//!
//! The feed DECLARES refs; it never calls `resolve_ref` and never peeks the
//! store by id (the #3083 cache-luck bug class this design forecloses).

use serde::{Deserialize, Serialize};

/// A closed typed pointer to an event or a replaceable/addressable coordinate.
///
/// Never a display string, never guessed from partial data — a `RenderOnly`
/// event-id target comes from a proven `e`/`q` tag or embedded event; a
/// `Delivered` address target comes from a proven `a`/uppercase-`A` coordinate.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(tag = "target_type", rename_all = "snake_case")]
pub enum TypedRefTarget {
    /// A concrete event id target.
    EventId(String),
    /// A replaceable/addressable target coordinate (`kind:pubkey:d`).
    Address {
        kind: u32,
        pubkey: String,
        d: String,
    },
}

impl TypedRefTarget {
    /// The event id, when this target names a concrete event (not an address).
    #[must_use]
    pub fn event_id(&self) -> Option<&str> {
        match self {
            Self::EventId(id) => Some(id.as_str()),
            Self::Address { .. } => None,
        }
    }

    /// The canonical opaque string key for this target — the event id, or the
    /// `kind:pubkey:d` coordinate string. A [`crate::composite`] lane mapping
    /// that declares a `Delivered` ref MUST use this SAME string as the
    /// [`crate::FeedRow::canonical_row_id`] it assigns, so the row the
    /// declaring lane surfaces and the row the delivered target itself
    /// surfaces collapse into ONE canonical row.
    #[must_use]
    pub fn canonical_key(&self) -> String {
        match self {
            Self::EventId(id) => id.clone(),
            Self::Address { kind, pubkey, d } => format!("{kind}:{pubkey}:{d}"),
        }
    }
}

/// How the engine treats a [`TypedRef`]'s target.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    /// Declared for lazy render-only resolution (`resolve_ref` / D7 lane).
    /// Never absorbed into this feed session's own acquisition/admission.
    RenderOnly,
    /// Absorbed into this feed session's own `live_shapes` + admission: the
    /// target re-enters as a real delivered event. At most one per row.
    Delivered,
}

/// A single delivery-tagged typed ref a [`crate::FeedRow`] declares.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct TypedRef {
    pub target: TypedRefTarget,
    pub delivery_mode: DeliveryMode,
}

impl TypedRef {
    /// Build a `RenderOnly` ref to an event id (the existing repost/quote
    /// render-target shape).
    #[must_use]
    pub fn render_only_event(id: impl Into<String>) -> Self {
        Self {
            target: TypedRefTarget::EventId(id.into()),
            delivery_mode: DeliveryMode::RenderOnly,
        }
    }

    /// Build a `Delivered` ref to an event id.
    #[must_use]
    pub fn delivered_event(id: impl Into<String>) -> Self {
        Self {
            target: TypedRefTarget::EventId(id.into()),
            delivery_mode: DeliveryMode::Delivered,
        }
    }

    /// Build a `Delivered` ref to a replaceable/addressable coordinate.
    #[must_use]
    pub fn delivered_address(kind: u32, pubkey: impl Into<String>, d: impl Into<String>) -> Self {
        Self {
            target: TypedRefTarget::Address {
                kind,
                pubkey: pubkey.into(),
                d: d.into(),
            },
            delivery_mode: DeliveryMode::Delivered,
        }
    }
}

/// Union two rows' ref vectors, deduping by target and preserving the "at most
/// one `Delivered` ref" invariant (later `Delivered` refs to a NEW target are
/// dropped rather than admitted as a second delivered target — a merge across
/// sources for the SAME canonical row must be about the SAME delivered thing).
#[must_use]
pub fn merge_refs(existing: &[TypedRef], incoming: &[TypedRef]) -> Vec<TypedRef> {
    let mut merged: Vec<TypedRef> = Vec::new();
    let mut has_delivered = false;
    for r in existing.iter().chain(incoming.iter()) {
        if merged.iter().any(|m| m.target == r.target) {
            continue;
        }
        if r.delivery_mode == DeliveryMode::Delivered {
            if has_delivered {
                continue;
            }
            has_delivered = true;
        }
        merged.push(r.clone());
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_dedupes_identical_targets() {
        let a = vec![TypedRef::render_only_event("x")];
        let b = vec![TypedRef::render_only_event("x")];
        assert_eq!(merge_refs(&a, &b), vec![TypedRef::render_only_event("x")]);
    }

    #[test]
    fn merge_keeps_at_most_one_delivered_ref() {
        let a = vec![TypedRef::delivered_event("first")];
        let b = vec![TypedRef::delivered_event("second")];
        let merged = merge_refs(&a, &b);
        assert_eq!(merged, vec![TypedRef::delivered_event("first")]);
    }

    #[test]
    fn merge_keeps_render_only_and_delivered_refs_to_different_targets() {
        let a = vec![TypedRef::render_only_event("preview")];
        let b = vec![TypedRef::delivered_event("root")];
        let merged = merge_refs(&a, &b);
        assert_eq!(merged.len(), 2);
    }
}
