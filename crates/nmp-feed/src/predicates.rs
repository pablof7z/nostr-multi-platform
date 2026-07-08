//! Generic feed predicate/closure aliases.
//!
//! These are kind-agnostic capability closures the composition root supplies to
//! the generic [`crate::FlatFeed`]. None of them name a protocol convention.
//!
//! They were relocated here from the demolished `root_indexed` engine module.
//! The names `RootAdmission` / `FollowPredicate` are retained (not renamed) to
//! keep the demolition PR's blast radius contained; a follow-up under #3082 may
//! rename `RootAdmission` → `AdmissionPredicate` once the reduced-source seam is
//! settled.

use std::sync::Arc;

use nmp_core::substrate::{EventId, KernelEvent};

/// Admission predicate (#1740 step 3): `true` for events allowed to enter the
/// feed. This is the compiled perspective gate — a `ContactList` / `ListMembers`
/// / `Wot` / `Difference` perspective filters the rendered feed itself.
///
/// It is EVENT-AWARE (not author-only) so author-scope perspectives and `#t`
/// tag-scope perspectives compose faithfully. Semantically identical to
/// [`crate::FlatFeedPredicate`]; the generic [`crate::FlatFeed`] admits on this.
pub type RootAdmission = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

/// Predicate over a pubkey the composition root supplies to decide whether a
/// referencing author qualifies for app-built attribution/rollup. The generic
/// feed engine no longer consumes this itself (reply-rollup was demolished);
/// reduced-source resolution still computes it for apps that want to build a
/// rollup on the four knobs.
pub type FollowPredicate = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Read-cache lookup by event id. An app-supplied, local-only cache read (never
/// an acquisition seam). The demolished engine used this for synchronous
/// repost/reply rekey (the order-dependent "cache-luck" bug, #3083); the
/// generic feed does NOT read the cache. Retained as a generic capability alias
/// for callers that resolve render targets lazily.
pub type EventLookup = Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>;

/// Admission for sources whose acquisition already gates every event.
#[must_use]
pub fn admit_all_roots() -> RootAdmission {
    Arc::new(|_event: &KernelEvent| true)
}
