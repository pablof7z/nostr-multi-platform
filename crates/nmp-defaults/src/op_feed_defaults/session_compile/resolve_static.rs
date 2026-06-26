//! Resolvers for the STATIC, fully-data-driven feed scopes (#1740) — those whose
//! acquisition + admission are fixed at declaration with NO reactive projection,
//! observer, or reset hook:
//!
//! * [`resolve_authors`] — `FeedScope::Authors { authors }`: the primary-kind
//!   timeline authored BY a fixed, app-named pubkey set (an author/profile feed).
//! * [`resolve_tag`] — `FeedScope::Tag { term }`: a `#t` scope.
//! * [`resolve_referrer`] — `FeedScope::Referrer { event_id }`: root-by-id plus
//!   `#e` referrers for a thread feed.
//!
//! Both compile to an EVENT-AWARE [`AdmitExpr`] admission (so they compose
//! faithfully under set algebra) over a fixed acquisition interest. The reactive
//! scopes (follow-set / list / WoT) live in [`super::resolve`].

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_feed::{AdmitExpr, RootAdmission};
use nmp_ffi::FeedOpenError;
use nmp_planner::InterestShape;

use super::resolve::not_supported;
use super::source::{
    empty_extra, AcquisitionInterest, LiveShape, OpSessionIdentity, ReducedSource,
};

// ── Authors { authors } — static author-set timeline ─────────────────────

/// A STATIC, app-named author set: the pubkeys are fixed at declaration, so the
/// admission is a fixed [`AdmitExpr::Authors`] and the acquisition is a fixed
/// author+kind timeline — no reactive projection, observer, or reset hook
/// (unlike the follow-set / list / WoT scopes whose membership grows live).
///
/// Fail-closed (D6): an EMPTY author set (or no acquisition kinds) is rejected
/// with a typed error BEFORE any acquisition is built — it must never degrade to
/// "admit everyone" / acquire nothing.
pub(super) fn resolve_authors(
    authors: &BTreeSet<String>,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    if authors.is_empty() {
        return Err(not_supported("Authors-empty-set"));
    }
    if kinds.is_empty() {
        return Err(not_supported("Authors-no-acquisition-kinds"));
    }

    // EVENT-AWARE admission over the fixed author set AND compiled kinds. The
    // acquisition filter carries the same kinds, but admission also has to gate
    // cached/live rows delivered through other paths.
    let admission: RootAdmission = {
        let authors = authors.clone();
        let kinds = kinds.clone();
        Arc::new(move |event: &nmp_core::substrate::KernelEvent| {
            authors.contains(&event.author) && kinds.contains(&event.kind)
        })
    };
    // Acquisition: the authors' primary-kind (+ compiler-derived wrapper) timeline.
    let shape = InterestShape::timeline_for(authors.clone(), kinds.clone());
    let interests = vec![AcquisitionInterest::global(shape.clone())];
    let live_shape: LiveShape = Arc::new(move || Some(shape.clone()));

    Ok(ReducedSource {
        op_session_identity: OpSessionIdentity::RequireActive,
        admission,
        interests,
        live_shape,
        extra_acquisition: empty_extra(),
        reset_hooks: Vec::new(),
        resolver_observer_ids: Vec::new(),
        identity_observer_ids: Vec::new(),
    })
}

// ── Tag { term } — #t scope, admit any acquired row ───────────────────────

pub(super) fn resolve_tag(term: &str, kinds: &BTreeSet<u32>) -> ReducedSource {
    // The #t filter gates at acquisition, but admission must be EVENT-AWARE
    // (`AdmitExpr::Tag`), not `Any` (#1740 step 3): so a `Tag` scope composes
    // faithfully inside set algebra (e.g. `Intersection(Tag, ContactList)`
    // checks BOTH the tag AND author membership instead of silently admitting
    // any member's untagged note).
    let admission: RootAdmission = AdmitExpr::Tag(term.to_string()).to_root_admission();
    let shape = tag_interest_shape(term, kinds);
    let interests = vec![AcquisitionInterest::global(shape.clone())];
    let live_shape: LiveShape = {
        let shape = tag_live_shape(term, kinds);
        Arc::new(move || shape.clone())
    };
    ReducedSource {
        op_session_identity: OpSessionIdentity::RequireActive,
        admission,
        interests,
        live_shape,
        // The #t acquisition is fully static (the fixed Global interest above);
        // nothing to re-sync.
        extra_acquisition: empty_extra(),
        reset_hooks: Vec::new(),
        resolver_observer_ids: Vec::new(),
        identity_observer_ids: Vec::new(),
    }
}

// ── Typed shape helpers ───────────────────────────────────────────────────

fn tag_interest_shape(term: &str, kinds: &BTreeSet<u32>) -> InterestShape {
    let mut shape = InterestShape {
        kinds: kinds.clone(),
        ..InterestShape::default()
    };
    shape
        .tags
        .insert("t".to_string(), [term.to_string()].into_iter().collect());
    shape
}

fn tag_live_shape(term: &str, kinds: &BTreeSet<u32>) -> Option<InterestShape> {
    if kinds.is_empty() {
        return None;
    }
    Some(tag_interest_shape(term, kinds))
}

// ── Referrer { event_id } — thread/referrer scope ──────────────────────────

/// A STATIC thread/referrer scope: the root event (by id) plus every primary-kind
/// event or derived wrapper that references the root via an `#e` tag. Fail-closed
/// on empty event_id or empty kinds (mirror resolve_authors).
pub(super) fn resolve_referrer(
    event_id: &str,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    if event_id.is_empty() {
        return Err(not_supported("Referrer-empty-id"));
    }
    if kinds.is_empty() {
        return Err(not_supported("Referrer-no-acquisition-kinds"));
    }

    // EVENT-AWARE admission: admits the root by id OR any acquired event that
    // references the root via an #e tag (the same predicate logic as
    // thread_feed_predicate in nmp-nip01).
    let root_id = event_id.to_string();
    let admission: RootAdmission = {
        let id = root_id.clone();
        let kinds = kinds.clone();
        Arc::new(move |event: &nmp_core::substrate::KernelEvent| {
            if !kinds.contains(&event.kind) {
                return false;
            }
            if event.id == id {
                return true;
            }
            event
                .tags
                .iter()
                .any(|tag| tag.first().map(String::as_str) == Some("e") && tag.get(1) == Some(&id))
        })
    };

    // Acquisition: two shapes — the #e reply-tail shape (for load_older), plus
    // the root-by-id shape (so the root note itself is replayed into the store).
    let interests = vec![
        AcquisitionInterest::global(referrer_shape(&root_id, kinds)),
        AcquisitionInterest::global(root_id_shape(&root_id, kinds)),
    ];

    // Live shape: the #e-covered reply-tail shape (re-read on load_older).
    let live_shape: LiveShape = {
        let id = root_id.clone();
        let k = kinds.clone();
        Arc::new(move || referrer_live_shape(&id, &k))
    };
    Ok(ReducedSource {
        op_session_identity: OpSessionIdentity::RequireActive,
        admission,
        interests,
        live_shape,
        extra_acquisition: empty_extra(),
        reset_hooks: Vec::new(),
        resolver_observer_ids: Vec::new(),
        identity_observer_ids: Vec::new(),
    })
}

// ── Typed shape helpers for referrer scope ────────────────────────────────

fn referrer_shape(root_id: &str, kinds: &BTreeSet<u32>) -> InterestShape {
    let mut shape = InterestShape {
        kinds: kinds.clone(),
        ..InterestShape::default()
    };
    shape
        .tags
        .insert("e".to_string(), [root_id.to_string()].into_iter().collect());
    shape
}

fn referrer_live_shape(root_id: &str, kinds: &BTreeSet<u32>) -> Option<InterestShape> {
    if kinds.is_empty() {
        return None;
    }
    Some(referrer_shape(root_id, kinds))
}

fn root_id_shape(root_id: &str, kinds: &BTreeSet<u32>) -> InterestShape {
    InterestShape {
        event_ids: [root_id.to_string()].into_iter().collect(),
        kinds: kinds.clone(),
        ..InterestShape::default()
    }
}
