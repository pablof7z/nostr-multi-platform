//! `FeedScope` → [`ReducedSource`] for the perspective compiler (#1740 step 3).
//!
//! This is the ONLY module that touches the resolution snapshots — kind:3 follows
//! ([`nmp_nip02::ActiveFollowSet`]), NIP-51 list members
//! ([`nmp_nip51::PeopleListProjection`]), and ranked WoT candidates (the #1698
//! [`nmp_wot::score::WotGraph`] query). It reuses those single-source mechanisms;
//! it never re-derives a follow graph or a list parser (D4).
//!
//! Each non-default scope resolves to a [`ReducedSource`]:
//! * `admission` — the engine's EVENT-AWARE [`nmp_feed::RootAdmission`], built
//!   INSIDE the framework from a [`nmp_feed::AdmitExpr`] (static sets / `#t` tag
//!   / set algebra) OR from a LIVE framework projection (reactive scopes). No
//!   app closure crosses FFI. It gates which roots ENTER the feed (#1740 step 3),
//!   not just reply attribution.
//! * `interests` — the internal typed acquisition interests.
//! * `live_shape` — the live pull acquisition shape (re-read on `load_older`).
//! * `reset_hooks` — closures that install a window-reset on each underlying
//!   set's change (reactive perspective), plus the observer ids to revoke.
//!
//! Deferred / fail-closed (typed `ScopeNotSupportedYet`, no registration):
//! * `RelaySet` — no framework relay-set-id resolver exists; relay-pinned
//!   acquisition is out of step-3 scope.
//! * `ContactList { owner }` for a FOREIGN owner — only the active viewer's
//!   kind:3 has a framework resolver; an arbitrary owner's contact list has no
//!   single-source projection. The active-owner case resolves via the follow set.
//! * `CustomPerspectiveId` — step 4 (the registration mechanism).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_feed::RootAdmission;
use nmp_ffi::{FeedOpenError, NmpApp};
use nmp_planner::InterestShape;
use nmp_wot::score::WotGraph;

use super::source::{
    extra_from_live_shape, AcquisitionInterest, ExtraAcquisition, LiveShape, ReducedSource,
    ResetHook,
};

const KIND_CONTACT_LIST: u32 = 3;
/// NIP-51 follow set / people list (kind:30000). Named locally because
/// `nmp-defaults` does not depend on `nmp-kinds`; the canonical constant is
/// `nmp_kinds::KIND_FOLLOW_SET` (the projection in `nmp-nip51` uses that).
const KIND_FOLLOW_SET: u32 = 30_000;
/// WoT ranked-candidate cap (the #1698 query takes a limit; 0 = unlimited).
const WOT_CANDIDATE_LIMIT: usize = 500;

/// Resolve a non-default, non-set-algebra scope. Set algebra is handled by
/// [`super::set_algebra`]; `ActiveUserFollows` / `CustomPerspectiveId` are
/// handled in `mod.rs`.
pub(super) fn resolve_scope(
    app: &NmpApp,
    scope: &nmp_feed::FeedScope,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    use nmp_feed::FeedScope as S;
    match scope {
        S::Authors { authors } => super::resolve_static::resolve_authors(authors, kinds),
        S::ContactList { owner } => resolve_contact_list(app, owner, kinds),
        S::ListMembers { list } => resolve_list_members(app, &list.0, kinds),
        S::Wot { seed, .. } => resolve_wot(app, &seed.0, kinds),
        S::Tag { term } => Ok(super::resolve_static::resolve_tag(&term.0, kinds)),
        S::Referrer { event_id } => super::resolve_static::resolve_referrer(event_id, kinds),
        S::RelaySet { .. } => Err(not_supported("RelaySet")),
        S::Union(l, r) => super::set_algebra::resolve_set_op(app, SetOp::Union, l, r, kinds),
        S::Intersection(l, r) => {
            super::set_algebra::resolve_set_op(app, SetOp::Intersection, l, r, kinds)
        }
        S::Difference(l, r) => {
            super::set_algebra::resolve_set_op(app, SetOp::Difference, l, r, kinds)
        }
        S::ActiveUserFollows | S::CustomPerspectiveId(_) => {
            // Handled by the dispatcher; unreachable here. Fail closed.
            Err(not_supported("scope-routing"))
        }
    }
}

/// The set-algebra operator, shared with `set_algebra.rs`.
#[derive(Clone, Copy)]
pub(super) enum SetOp {
    Union,
    Intersection,
    Difference,
}

pub(super) fn not_supported(scope: &'static str) -> FeedOpenError {
    FeedOpenError::ScopeNotSupportedYet { scope }
}

// ── ContactList { owner } ────────────────────────────────────────────────

/// The active viewer's contact list resolves via the framework
/// [`nmp_nip02::ActiveFollowSet`] (live, reactive). A FOREIGN owner's kind:3 has
/// no single-source framework resolver → fail closed (deferred to step 4).
fn resolve_contact_list(
    app: &NmpApp,
    owner: &str,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    let viewer = super::super::read_active(&app.active_account_handle())
        .ok_or_else(|| not_supported("ContactList-no-active-account"))?;
    if owner != viewer {
        return Err(not_supported("ContactList-foreign-owner"));
    }

    // A fresh ActiveFollowSet over the same active-account slot, registered as a
    // session observer so kind:3 ingest keeps the predicate live (reactive).
    let follow_set = nmp_nip02::ActiveFollowSet::new(app.active_account_handle());
    let observer_id =
        app.register_event_observer(Arc::clone(&follow_set) as Arc<dyn KernelEventObserver>);

    // Event-aware over the author-only follow predicate: a root is admitted iff
    // its author is in the live follow set.
    let admission: RootAdmission = {
        let pred = follow_set.predicate();
        Arc::new(move |event: &KernelEvent| pred(&event.author))
    };
    let live_shape = follow_set_live_shape(&app.active_account_handle(), &follow_set, kinds);
    // No fixed member interest: the follow set grows as kind:3 arrives, so the
    // members' timeline acquisition is re-synced live (extra_acquisition).
    let interests = Vec::new();

    let reset_set = Arc::clone(&follow_set);
    let reset_hook: ResetHook = Box::new(move |reset| {
        reset_set.on_change(Box::new(move || reset()));
    });

    Ok(ReducedSource {
        admission,
        interests,
        extra_acquisition: extra_from_live_shape(&live_shape),
        live_shape,
        reset_hooks: vec![reset_hook],
        resolver_observer_ids: vec![observer_id],
    })
}

// ── ListMembers { list } (NIP-51 kind:30000) ─────────────────────────────

fn resolve_list_members(
    app: &NmpApp,
    list_id: &str,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    // The list owner is the active viewer (the projection is owner-gated). No
    // active account ⇒ fail closed (no list to resolve).
    let viewer = super::super::read_active(&app.active_account_handle())
        .ok_or_else(|| not_supported("ListMembers-no-active-account"))?;

    let projection = Arc::new(nmp_nip51::PeopleListProjection::new(
        app.active_account_handle(),
    ));
    let observer_id =
        app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);

    // LIVE predicate over the projection's current members (reactive: a new
    // kind:30000 updates the set and fires on_change → window reset).
    let admission: RootAdmission = {
        let projection = Arc::clone(&projection);
        let list_id = list_id.to_string();
        Arc::new(move |event: &KernelEvent| projection.members(&list_id).contains(&event.author))
    };

    // Acquire the viewer's kind:30000 list event. The members' timeline is
    // re-synced live by the session engine as the list arrives + changes (the
    // member set is empty until the list lands).
    let interests = vec![AcquisitionInterest::active_account(viewer_list_shape(
        &viewer,
    ))];

    let live_shape: LiveShape = {
        let projection = Arc::clone(&projection);
        let list_id = list_id.to_string();
        let kinds = kinds.clone();
        Arc::new(move || {
            let members = projection.members(&list_id);
            if members.is_empty() || kinds.is_empty() {
                return None;
            }
            Some(InterestShape::timeline_for(
                members.into_iter().collect(),
                kinds.clone(),
            ))
        })
    };

    let reset_proj = Arc::clone(&projection);
    let reset_hook: ResetHook = Box::new(move |reset| {
        reset_proj.on_change(Box::new(move || reset()));
    });

    Ok(ReducedSource {
        admission,
        interests,
        extra_acquisition: extra_from_live_shape(&live_shape),
        live_shape,
        reset_hooks: vec![reset_hook],
        resolver_observer_ids: vec![observer_id],
    })
}

// ── Wot { seed, rules } — reuse the #1698 ranked query ────────────────────

fn resolve_wot(
    app: &NmpApp,
    seed: &str,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    // A session-scoped WoT graph observer, reusing `WotGraph` (the #1698 ranked
    // second-degree query) — we do NOT touch the singleton bootstrap runtime.
    let graph = Arc::new(SessionWotGraph::new(seed.to_string()));
    let observer_id =
        app.register_event_observer(Arc::clone(&graph) as Arc<dyn KernelEventObserver>);

    let admission: RootAdmission = {
        let graph = Arc::clone(&graph);
        Arc::new(move |event: &KernelEvent| graph.admits(&event.author))
    };

    // Acquire the seed's contact list (kind:3). The seed's DIRECT follows' kind:3
    // (needed to rank second-degree candidates) and the candidates' timelines are
    // re-synced live by the session engine as the graph fills (extra_acquisition +
    // live_shape below).
    let interests = vec![AcquisitionInterest::active_account(seed_contacts_shape(
        seed,
    ))];

    let live_shape: LiveShape = {
        let graph = Arc::clone(&graph);
        let kinds = kinds.clone();
        Arc::new(move || {
            let candidates = graph.ranked_candidates();
            if candidates.is_empty() || kinds.is_empty() {
                return None;
            }
            Some(InterestShape::timeline_for(
                candidates.into_iter().collect(),
                kinds.clone(),
            ))
        })
    };

    // The second-degree ranking needs each direct follow's kind:3 contact list,
    // and the candidates' primary-kind timeline must be acquired once ranked.
    // Acquire both live as the graph fills (seed kind:3 → direct follows known →
    // fetch their kind:3 → candidates rank → fetch their timelines).
    let extra_acquisition: ExtraAcquisition = {
        let graph = Arc::clone(&graph);
        let timeline_kinds = kinds.clone();
        Arc::new(move || {
            let mut shapes = Vec::new();
            let follows = graph.direct_follows();
            if !follows.is_empty() {
                let k: BTreeSet<u32> = [KIND_CONTACT_LIST].into_iter().collect();
                shapes.push(AcquisitionInterest::active_account(
                    InterestShape::timeline_for(follows, k),
                ));
            }
            let candidates = graph.ranked_candidates();
            if !candidates.is_empty() && !timeline_kinds.is_empty() {
                shapes.push(AcquisitionInterest::active_account(
                    InterestShape::timeline_for(
                        candidates.into_iter().collect(),
                        timeline_kinds.clone(),
                    ),
                ));
            }
            shapes
        })
    };

    let reset_graph = Arc::clone(&graph);
    let reset_hook: ResetHook = Box::new(move |reset| {
        reset_graph.on_change(Box::new(move || reset()));
    });

    Ok(ReducedSource {
        admission,
        interests,
        live_shape,
        extra_acquisition,
        reset_hooks: vec![reset_hook],
        resolver_observer_ids: vec![observer_id],
    })
}

// ── Typed acquisition shape helpers ───────────────────────────────────────

fn viewer_list_shape(viewer: &str) -> InterestShape {
    InterestShape::timeline_for(
        [viewer.to_string()].into_iter().collect(),
        [KIND_FOLLOW_SET].into_iter().collect(),
    )
}

fn seed_contacts_shape(seed: &str) -> InterestShape {
    InterestShape::timeline_for(
        [seed.to_string()].into_iter().collect(),
        [KIND_CONTACT_LIST].into_iter().collect(),
    )
}

fn follow_set_live_shape(
    slot: &nmp_core::slots::ActiveAccountSlot,
    follow_set: &Arc<nmp_nip02::ActiveFollowSet>,
    kinds: &BTreeSet<u32>,
) -> LiveShape {
    let slot = slot.clone();
    let follow_set = Arc::clone(follow_set);
    let kinds = kinds.clone();
    Arc::new(move || {
        if kinds.is_empty() {
            return None;
        }
        let viewer = super::super::read_active(&slot)?;
        let mut authors: BTreeSet<String> = follow_set.follows().into_iter().collect();
        authors.insert(viewer);
        Some(InterestShape::timeline_for(authors, kinds.clone()))
    })
}

// ── Session-scoped WoT graph observer (reuses WotGraph ranked query) ───────

/// A minimal kind:3-ingesting WoT graph for ONE feed session, reusing
/// [`nmp_wot::score::WotGraph`]'s ranked second-degree query (#1698). It does
/// not duplicate the ranking logic — it owns a `WotGraph`, feeds it kind:3
/// edges, and reads `ranked_second_degree_candidates`.
pub(super) struct SessionWotGraph {
    seed: String,
    graph: Mutex<WotGraph>,
    /// The seed's DIRECT follows (from the seed's own kind:3), tracked so the
    /// session can acquire their contact lists for second-degree ranking.
    direct: Mutex<BTreeSet<String>>,
    /// Cached ranked candidate set — recomputed once per graph change, not once
    /// per admission test (the predicate is hit once per candidate root).
    ranked: Mutex<BTreeSet<String>>,
    on_change: Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
}

impl SessionWotGraph {
    pub(super) fn new(seed: String) -> Self {
        Self {
            seed,
            graph: Mutex::new(WotGraph::default()),
            direct: Mutex::new(BTreeSet::new()),
            ranked: Mutex::new(BTreeSet::new()),
            on_change: Mutex::new(Vec::new()),
        }
    }

    /// The current ranked second-degree candidate set (cached).
    pub(super) fn ranked_candidates(&self) -> BTreeSet<String> {
        self.ranked.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// The seed's direct follows (their kind:3 feeds the ranking).
    pub(super) fn direct_follows(&self) -> BTreeSet<String> {
        self.direct.lock().map(|d| d.clone()).unwrap_or_default()
    }

    pub(super) fn admits(&self, pk: &str) -> bool {
        self.ranked.lock().map(|r| r.contains(pk)).unwrap_or(false)
    }

    fn on_change(&self, cb: Box<dyn Fn() + Send + Sync>) {
        if let Ok(mut cbs) = self.on_change.lock() {
            cbs.push(cb);
        }
    }

    fn fire(&self) {
        if let Ok(cbs) = self.on_change.lock() {
            for cb in cbs.iter() {
                cb();
            }
        }
    }
}

impl KernelEventObserver for SessionWotGraph {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_CONTACT_LIST {
            return;
        }
        // Track the seed's direct follows from the seed's own kind:3.
        if event.author == self.seed {
            let follows: BTreeSet<String> = event
                .tags
                .iter()
                .filter_map(|tag| {
                    if tag.first().is_some_and(|t| t == "p") {
                        tag.get(1).cloned()
                    } else {
                        None
                    }
                })
                .collect();
            if let Ok(mut direct) = self.direct.lock() {
                *direct = follows;
            }
        }
        // Ingest the edge and recompute the cached ranked set ONCE per change.
        let ranked: BTreeSet<String> = {
            let Ok(mut graph) = self.graph.lock() else {
                return;
            };
            graph.ingest_event(&event.author, event.kind, &event.tags);
            graph
                .ranked_second_degree_candidates(&self.seed, WOT_CANDIDATE_LIMIT)
                .into_iter()
                .map(|(pk, _)| pk)
                .collect()
        };
        if let Ok(mut cache) = self.ranked.lock() {
            *cache = ranked;
        }
        self.fire();
    }
}
