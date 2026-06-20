//! The [`RootIndexedFeed`] ingest state machine — the per-event decision logic
//! and the buffered-attribution bookkeeping. Split from `engine/mod.rs` to keep
//! each file under the 500-LOC ceiling; this is a continuation `impl` block on
//! the same type plus its free helpers.

use nmp_core::substrate::{BoundedMessageMap, EventId, KernelEvent};
use nmp_threading::{pointer::ThreadPointer, ParentResolver};

use super::{EngineState, RootIndexedFeed, RootSlot, MAX_ATTRIBUTION_PER_ROOT};
use crate::root_indexed::attribution::AttributionPayload;

impl<R, A, C> RootIndexedFeed<R, A, C>
where
    R: ParentResolver,
    A: AttributionPayload + serde::Serialize,
    C: Clone + Send + Sync + serde::Serialize,
{
    /// Ingest one accepted `KernelEvent`. The observer impl calls this.
    /// Resilient to a poisoned lock (D6: drop the event rather than panic on
    /// the actor thread).
    pub(super) fn ingest(&self, event: &KernelEvent) {
        // Fast gate: drop non-feed-eligible kinds before touching any state.
        // The predicate is caller-supplied so the engine stays kind-agnostic (D0).
        if !(self.caps.event_gate)(event) {
            return;
        }

        if let Some(target) = self.caps.resolver.supersedes(event) {
            self.ingest_repost(event, target);
            return;
        }

        match self.caps.resolver.parent(event) {
            // Root-shaped: no parent edge → this is an OP.
            None => self.ingest_root(event),
            // Reply-shaped: only followed authors qualify as attribution.
            Some(pointer) => {
                if (self.caps.follow)(&event.author) {
                    self.ingest_reply(event, pointer);
                }
                // Non-follow replies are dropped (no state change).
            }
        }
    }

    /// Insert a root and drain any buffered attributions for it.
    ///
    /// L-5: when a repost wrapper already keyed this id (`supersedes_target`
    /// set, empty/placeholder card), the arriving target rebuilds the card body
    /// **without losing the repost provenance** — the existing
    /// `supersedes_target` is preserved so the renderer still shows the
    /// "reposted by" banner. A plain (non-reposted) root just inserts.
    fn ingest_root(&self, event: &KernelEvent) {
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        let existing = st.roots.get(&event.id).map(|slot| {
            (
                slot.supersedes_target.clone(),
                slot.wrapper_event_id.clone(),
                slot.created_at,
            )
        });
        let (supersedes_target, wrapper_event_id, prior_created_at) = match existing {
            Some((target, wrapper, created)) => (target, wrapper, Some(created)),
            None => (None, None, None),
        };
        // L-5 late-target path: a repost wrapper keyed this id first. Re-fetch the
        // wrapper so the card is rebuilt from the `(wrapper, target)` pair,
        // preserving repost provenance. Plain roots build from `(event, None)`.
        let wrapper = wrapper_event_id
            .as_ref()
            .and_then(|id| (self.caps.event_lookup)(id));
        let card = match &wrapper {
            Some(wrapper_event) => (self.caps.card_builder)(wrapper_event, Some(event)),
            None => (self.caps.card_builder)(event, None),
        };
        let created_at = prior_created_at.map_or(event.created_at, |c| c.max(event.created_at));
        // Fix 4: when the bounded roots map is at capacity the oldest root is
        // evicted on insert. Its attribution sub-map must be reclaimed too,
        // otherwise `attributions` grows without bound as live roots crowd out
        // the entries of evicted roots.
        let (_, evicted) = st.roots.insert_returning_evicted(
            event.id.clone(),
            RootSlot {
                card,
                created_at,
                supersedes_target,
                wrapper_event_id,
            },
        );
        if let Some((evicted_id, _)) = evicted {
            st.attributions.remove(&evicted_id);
        }
        Self::drain_pending_into(&mut st, &event.id);
    }

    /// Repost-shaped event (`supersedes == Some(target)`): the target becomes
    /// the surfaced root. Insert the wrapper card keyed by the target id;
    /// rebuild from the pair if already local. If the target is absent, keep a
    /// structural placeholder. Target fetching belongs to the mounted component
    /// that wants to render that target.
    fn ingest_repost(&self, wrapper: &KernelEvent, target: EventId) {
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        let target_event = (self.caps.event_lookup)(&target);
        let card = (self.caps.card_builder)(wrapper, target_event.as_ref());
        // Fix 1: never regress a slot's created_at — an older repost wrapper
        // must not pull an existing root downward in feed order. (The root
        // path in `ingest_root` already takes this max; reposts must too.)
        let created_at = match st.roots.get(&target).map(|s| s.created_at) {
            Some(existing) => existing.max(wrapper.created_at),
            None => wrapper.created_at,
        };
        // Fix 4: reclaim the attribution sub-map of any root evicted by this
        // insert when the bounded roots map is at capacity (see ingest_root).
        let (_, evicted) = st.roots.insert_returning_evicted(
            target.clone(),
            RootSlot {
                card,
                created_at,
                supersedes_target: Some(target.clone()),
                wrapper_event_id: Some(wrapper.id.clone()),
            },
        );
        if let Some((evicted_id, _)) = evicted {
            st.attributions.remove(&evicted_id);
        }
        Self::drain_pending_into(&mut st, &target);
    }

    /// Reply from a followed author. Resolve the referenced root, re-key past a
    /// repost wrapper if applicable (L-2), and record the attribution against
    /// the root or buffer it until the root arrives through normal ingest.
    fn ingest_reply(&self, event: &KernelEvent, pointer: ThreadPointer) {
        // Prefer the explicit root pointer; fall back to the parent pointer.
        let resolved = self.caps.resolver.root(event).unwrap_or(pointer);

        let primary_id: EventId = match &resolved {
            ThreadPointer::Event { id, .. } => {
                // L-2: the reply targets a repost wrapper that is locally known
                // and supersedes a different id → re-key to that target.
                let rekeyed = (self.caps.event_lookup)(id)
                    .and_then(|parent| self.caps.resolver.supersedes(&parent));
                match rekeyed {
                    Some(target) => target,
                    None => id.clone(),
                }
            }
            ThreadPointer::Address { coord, .. } => coord.clone(),
            ThreadPointer::External { uri } => external_surrogate(uri),
        };

        let Some(attribution) = A::from_reply(event, self.caps.follow.as_ref()) else {
            return;
        };

        let Ok(mut st) = self.state.lock() else {
            return;
        };
        if st.roots.contains_key(&primary_id) {
            Self::record_attribution(&mut st.attributions, &primary_id, attribution);
        } else {
            Self::record_attribution(&mut st.pending_attributions, &primary_id, attribution);
        }
    }

    /// Drain buffered attributions for `root_id` into the live map. Called once
    /// the root is locally held.
    fn drain_pending_into(st: &mut EngineState<A, C>, root_id: &str) {
        if let Some(pending) = st.pending_attributions.remove(root_id) {
            let live = st
                .attributions
                .entry_or_insert_with(root_id.to_string(), || {
                    BoundedMessageMap::new(MAX_ATTRIBUTION_PER_ROOT)
                });
            for (reply_id, attribution) in pending.iter() {
                live.insert(reply_id.clone(), attribution.clone());
            }
        }
    }

    fn record_attribution(
        map: &mut BoundedMessageMap<EventId, BoundedMessageMap<EventId, A>>,
        root_id: &str,
        attribution: A,
    ) {
        let sub = map.entry_or_insert_with(root_id.to_string(), || {
            BoundedMessageMap::new(MAX_ATTRIBUTION_PER_ROOT)
        });
        // Per-root D5: when the sub-map is full a NEW reply id evicts the
        // oldest-inserted reply. We do NOT emit Release here — the root is
        // still referenced by the surviving attributions.
        sub.insert(attribution.reply_event_id().to_string(), attribution);
    }
}

/// Stable surrogate id for an external (non-Nostr) root reference. Lets
/// attribution attach even though the engine never hydrates it. The
/// `external:` prefix guarantees it never collides with a 64-hex event id.
fn external_surrogate(uri: &str) -> EventId {
    format!("external:{uri}")
}
