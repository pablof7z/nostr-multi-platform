//! [`RootIndexedFeed`] — the generic OP-centric root-indexed feed engine.
//!
//! Consumes `KernelEvent`s through the kernel observer fan-out and produces a
//! feed of **thread roots only**, each carrying the raw list of attributions
//! (qualifying references from followed authors). Unknown roots remain
//! structural placeholders until some other mounted component or protocol
//! module claims and delivers the referenced event; generic feed mechanics do
//! not fetch secondary data.
//!
//! This crate is substrate-generic: it names no protocol convention. The
//! resolver (`R: ParentResolver`) decides parent/root/supersedes edges; the
//! payload (`A: AttributionPayload`) decides what qualifies as attribution; the
//! follow predicate and event lookup are plain closures. A CI grep gate enforces
//! zero NIP tokens.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use nmp_core::substrate::{BoundedMessageMap, EventId, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_threading::ParentResolver;

use crate::root_indexed::attribution::AttributionPayload;
use crate::root_indexed::card::{RootCard, RootFeedSnapshot};
use crate::{FeedCursor, FeedPage, FeedRequest, FeedWindowPolicy};

/// The per-event ingest state machine lives in a sibling file to keep both
/// under the 500-LOC ceiling; it is a continuation `impl` on `RootIndexedFeed`.
mod ingest;

/// Per-root D5 cap: at most this many attributions per root sub-map.
/// Independent of the global [`MAX_PROJECTION_MESSAGES`] outer cap.
pub const MAX_ATTRIBUTION_PER_ROOT: usize = 64;

/// Predicate the engine consults to decide whether a referencing author's
/// reference qualifies as attribution (the follow-set membership test, wired by
/// the composition root — never a trait, never planner-coupled; D7).
pub type FollowPredicate = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Read-cache lookup the engine uses for repost L-2 / L-5 rebuild.
pub type EventLookup = Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>;

/// Gate predicate: `true` for feed-eligible event kinds (roots or attributions).
/// Events that fail the gate are dropped at the observer entry point before any
/// state is touched. Caller-supplied so the engine stays kind-agnostic (D0).
pub type EventGate = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

/// ROOT-admission predicate (#1740 step 3): `true` for events allowed to enter
/// the feed AS ROOTS. This is the compiled perspective gate — a `ContactList` /
/// `ListMembers` / `Wot` / `Difference` perspective must filter the rendered feed
/// itself, not merely its reply attributions.
///
/// It is EVENT-AWARE (not author-only) so author-scope perspectives and
/// `#t` tag-scope perspectives compose faithfully: `Intersection(Tag,
/// ContactList)` checks BOTH the event's author membership AND its `#t` tags.
/// Active-follows sessions can pass [`admit_all_roots`] when acquisition already
/// gates which roots arrive; scoped sessions pass their compiled predicate.
pub type RootAdmission = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

/// Root admission for sources whose acquisition already gates every root.
///
/// Active-follows feeds use this for the root lane: followed authors' timelines
/// are selected by acquisition, while the `FollowPredicate` gates reply
/// attribution.
#[must_use]
pub fn admit_all_roots() -> RootAdmission {
    Arc::new(|_event: &KernelEvent| true)
}

/// Build a render card from a root event, plus the supersedes-target event when
/// present (L-5 late-target rebuilds with both).
pub type CardBuilder<C> = Box<dyn Fn(&KernelEvent, Option<&KernelEvent>) -> C + Send + Sync>;

/// A locally-held root and its render card + bookkeeping for repost rebuild.
struct RootSlot<C> {
    card: C,
    created_at: u64,
    /// When this root is a repost-style wrapper, the id it supersedes. Used to
    /// rebuild the card while preserving repost provenance once the wrapped
    /// target arrives (L-5).
    supersedes_target: Option<EventId>,
    /// The kind:6 repost wrapper event id, when this slot was seeded by a
    /// repost. On L-5 late-target rebuild (target arrives after the wrapper),
    /// the engine re-fetches the wrapper via `event_lookup` and rebuilds the
    /// card from the `(wrapper, target)` pair so a renderer can still show the
    /// "reposted by" provenance. `None` for plain roots.
    wrapper_event_id: Option<EventId>,
}

/// Closure capability bundle, all shared/owned closures. Held outside the
/// `Mutex` (they are immutable after construction) so the hot observer path
/// does not contend on capability access.
struct Capabilities<R, C> {
    resolver: R,
    follow: FollowPredicate,
    /// The compiled perspective gate for ROOT insertion (#1740 step 3). A root
    /// whose event is not admitted here never enters the feed.
    root_admission: RootAdmission,
    event_gate: EventGate,
    event_lookup: EventLookup,
    card_builder: CardBuilder<C>,
}

/// Mutable engine state. One `Mutex` guards all maps; the observer path and
/// the snapshot path both take it. The capability closures live outside the
/// lock.
struct EngineState<A: AttributionPayload, C> {
    /// Locally-held roots → render card + bookkeeping. Outer D5 cap.
    roots: BoundedMessageMap<EventId, RootSlot<C>>,
    /// root_id → (reply_event_id → attribution). Per-root sub-map D5 capped.
    attributions: BoundedMessageMap<EventId, BoundedMessageMap<EventId, A>>,
    /// Buffered attributions for roots not yet locally held, keyed by the
    /// referenced root id. Drained when the root arrives through normal ingest.
    pending_attributions: BoundedMessageMap<EventId, BoundedMessageMap<EventId, A>>,
}

impl<A: AttributionPayload, C> EngineState<A, C> {
    fn new() -> Self {
        Self {
            roots: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
            attributions: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
            pending_attributions: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
        }
    }
}

/// The generic OP-centric feed engine.
///
/// `R` resolves thread edges; `A` is the attribution payload; `C` is the
/// render card produced by `card_builder` and stored on each [`RootCard`].
pub struct RootIndexedFeed<R, A, C>
where
    R: ParentResolver,
    A: AttributionPayload,
    C: Clone + Send + Sync + serde::Serialize,
{
    caps: Capabilities<R, C>,
    state: Mutex<EngineState<A, C>>,
    /// Current visible-window limit — the **render viewport**, grown one page at
    /// a time by [`Self::grow_visible_window`].
    ///
    /// ADR-0058 §8 step-6B: this is a pure render-ordering viewport, NOT a paging
    /// source of truth. Completeness rides the ingest-seq pull pager
    /// (`crate::PullFeedController`); the engine is no longer itself a
    /// `FeedController`. The viewport grows only as a *consequence* of a
    /// successful pull drain (the controller's `advance` step), revealing the
    /// `(created_at, id)`-sorted roots the pull ingested. There is no standalone
    /// `created_at` window-grow `load_older` path anymore — it was deleted in 6B.
    /// Held outside the `Mutex` (a plain monotone counter) so `snapshot_current_window`
    /// can read it without taking the state lock twice.
    window_policy: FeedWindowPolicy,
    window_limit: AtomicUsize,
}

impl<R, A, C> RootIndexedFeed<R, A, C>
where
    R: ParentResolver,
    A: AttributionPayload + serde::Serialize,
    C: Clone + Send + Sync + serde::Serialize,
{
    /// Construct the engine from its resolver and closure capabilities.
    ///
    /// * `follow` — true for pubkeys whose references qualify as attribution.
    /// * `event_gate` — true for feed-eligible kinds; events that fail the gate
    ///   are dropped at the observer entry point before any state is touched.
    /// * `event_lookup` — read-cache lookup, needed for repost L-2/L-5 rebuild.
    /// Missing roots or repost targets stay as structural placeholders. A UI
    /// component that wants to render the target must claim it through that
    /// component's own dependency path.
    /// * `root_admission` — the compiled perspective gate for ROOT insertion;
    ///   pass [`admit_all_roots`] when acquisition gates roots or the session's
    ///   compiled perspective predicate for a scoped feed.
    /// * `card_builder` — `(root_event, Option<target_event>) -> C`.
    pub fn new(
        resolver: R,
        follow: FollowPredicate,
        root_admission: RootAdmission,
        event_gate: EventGate,
        event_lookup: EventLookup,
        card_builder: CardBuilder<C>,
    ) -> Self {
        Self::new_with_window_policy(
            resolver,
            follow,
            root_admission,
            event_gate,
            event_lookup,
            card_builder,
            FeedWindowPolicy::default(),
        )
    }

    /// Construct the engine with an explicit app-declared window policy.
    #[must_use]
    pub fn new_with_window_policy(
        resolver: R,
        follow: FollowPredicate,
        root_admission: RootAdmission,
        event_gate: EventGate,
        event_lookup: EventLookup,
        card_builder: CardBuilder<C>,
        window_policy: FeedWindowPolicy,
    ) -> Self {
        let initial_limit = window_policy.initial_visible_limit();
        Self {
            caps: Capabilities {
                resolver,
                follow,
                root_admission,
                event_gate,
                event_lookup,
                card_builder,
            },
            state: Mutex::new(EngineState::new()),
            window_policy,
            window_limit: AtomicUsize::new(initial_limit),
        }
    }

    /// Tear down all state owned by the current feed perspective.
    ///
    /// A perspective is the caller-supplied admission/order source: active
    /// account, follow set, mute/block policy, relay set, WoT filter, or any
    /// equivalent app-defined view. When it changes, old rows must disappear
    /// immediately instead of aging out naturally.
    pub fn reset_for_perspective_change(&self) {
        if let Ok(mut st) = self.state.lock() {
            *st = EngineState::new();
        }

        let current = self.window_limit.load(Ordering::Relaxed);
        self.window_limit.store(
            self.window_policy.reset_visible_limit(current),
            Ordering::Relaxed,
        );
    }

    /// Remove one surfaced root and any attribution state keyed to it.
    ///
    /// The caller decides *why* the root is no longer admissible: delete,
    /// mute/block policy, app filter, or perspective-specific pruning. The
    /// generic engine only owns the mechanical removal.
    pub fn remove_root(&self, root_id: &str) -> bool {
        self.remove_root_if(root_id, |_| true)
    }

    /// Remove one surfaced root only when `predicate` accepts its card.
    ///
    /// Used by protocol adapters that need to validate a delete/suppression
    /// against card data before mutating feed state.
    pub fn remove_root_if(&self, root_id: &str, predicate: impl FnOnce(&C) -> bool) -> bool {
        let Ok(mut st) = self.state.lock() else {
            return false;
        };
        let should_remove = st
            .roots
            .get(root_id)
            .map(|slot| predicate(&slot.card))
            .unwrap_or(false);
        if !should_remove {
            return false;
        }
        st.roots.remove(root_id);
        st.attributions.remove(root_id);
        st.pending_attributions.remove(root_id);
        true
    }

    /// Remove the root that a repost **wrapper** seeded, keyed by the wrapper's
    /// own event id (not the wrapped target id).
    ///
    /// A row surfaced by a kind:6 repost is keyed by its *target* id, but the
    /// wrapper that surfaced it is tracked in [`RootSlot::wrapper_event_id`]. A
    /// NIP-09 kind:5 deletion that names the *wrapper* id must therefore find the
    /// root by wrapper id and remove it; otherwise a deleted repost keeps
    /// surfacing an out-of-perspective target. `predicate` validates the card
    /// (e.g. delete-author ownership) before removal. Returns the number of
    /// roots removed (0 or more; one wrapper seeds at most one root).
    pub fn remove_root_by_wrapper_if(
        &self,
        wrapper_event_id: &str,
        predicate: impl Fn(&C) -> bool,
    ) -> usize {
        let Ok(mut st) = self.state.lock() else {
            return 0;
        };
        let targets: Vec<String> = st
            .roots
            .iter()
            .filter(|(_, slot)| {
                slot.wrapper_event_id.as_deref() == Some(wrapper_event_id) && predicate(&slot.card)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &targets {
            st.roots.remove(id);
            st.attributions.remove(id);
            st.pending_attributions.remove(id);
        }
        targets.len()
    }

    /// Grow the **render viewport** by one page, revealing more of the
    /// `(created_at, id)`-sorted roots already ingested.
    ///
    /// ADR-0058 §8 step-6B: this is the viewport step of the single pull paging
    /// path — it is called ONLY by [`crate::PullFeedController`] after a
    /// successful seq-ordered pull drain has ingested a page of (possibly older)
    /// events through [`nmp_core::ObservedProjectionSink::on_kernel_event`]. It is
    /// NOT a standalone `created_at` window-grow `load_older` (that parallel path
    /// was deleted in 6B; the engine is no longer a `FeedController`).
    ///
    /// Returns `true` when the viewport actually grew (there were more roots to
    /// reveal and the hard ceiling was not yet hit), `false` when everything is
    /// already visible or the cap is reached.
    pub fn grow_visible_window(&self) -> bool {
        let total = self.state.lock().map(|st| st.roots.len()).unwrap_or(0);
        let current_limit = self.window_limit.load(Ordering::Relaxed);
        self.window_policy
            .next_visible_limit(current_limit, total)
            .is_some_and(|new_limit| {
                self.window_limit.store(new_limit, Ordering::Relaxed);
                true
            })
    }

    /// Build the visible-window snapshot using the engine's current render
    /// viewport limit. This honors any prior [`Self::grow_visible_window`] call
    /// that widened the viewport beyond `DEFAULT_FEED_WINDOW_LIMIT`.
    #[must_use]
    pub fn snapshot_current_window(&self) -> RootFeedSnapshot<C, A> {
        let limit = self.window_limit.load(Ordering::Relaxed);
        self.snapshot(&FeedRequest::newest(limit))
    }

    /// Build the visible-window snapshot: cards newest-first, windowed to the
    /// request limit (D5). Attribution vectors are raw (Q1).
    #[must_use]
    pub fn snapshot(&self, request: &FeedRequest) -> RootFeedSnapshot<C, A> {
        let Ok(st) = self.state.lock() else {
            return RootFeedSnapshot {
                cards: Vec::new(),
                page: None,
                metrics: None,
            };
        };
        // Order roots newest-first by (created_at, id).
        let mut ordered: Vec<(u64, EventId)> = st
            .roots
            .iter()
            .map(|(id, slot)| (slot.created_at, id.clone()))
            .collect();
        ordered.sort_by(|(lt, lid), (rt, rid)| rt.cmp(lt).then_with(|| rid.cmp(lid)));

        let limit = request.bounded_limit();
        let total = ordered.len();
        let end = limit.min(total);
        let has_more = end < total;
        let next_cursor = if has_more {
            ordered.get(end - 1).map(|(created_at, id)| FeedCursor {
                created_at: *created_at,
                id: id.clone(),
            })
        } else {
            None
        };

        let cards = ordered[..end]
            .iter()
            .filter_map(|(_, id)| {
                let slot = st.roots.get(id)?;
                let attribution = st
                    .attributions
                    .get(id)
                    .map(|sub| sub.values().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                Some(RootCard {
                    card: slot.card.clone(),
                    attribution,
                })
            })
            .collect::<Vec<_>>();

        RootFeedSnapshot {
            cards,
            page: Some(FeedPage {
                limit,
                next_cursor,
                has_more,
                total_blocks: total,
            }),
            metrics: None,
        }
    }
}

impl<R, A, C> nmp_core::ObservedProjectionSink for RootIndexedFeed<R, A, C>
where
    R: ParentResolver,
    A: AttributionPayload + serde::Serialize,
    C: Clone + Send + Sync + serde::Serialize,
{
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ingest(event);
    }
}
