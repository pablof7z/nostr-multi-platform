//! The [`RootIndexedFeed`] ingest state machine — the per-event decision logic
//! and the buffered-attribution bookkeeping. Split from `engine/mod.rs` to keep
//! each file under the 500-LOC ceiling; this is a continuation `impl` block on
//! the same type plus its free helpers.

use nmp_core::planner::RelayHint;
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

        // Profile events are handled first and short-circuit: a profile is not
        // a feed event.
        if let Some((pubkey, profile)) = (self.caps.profile_detector)(event) {
            self.apply_profile(&pubkey, profile);
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

    /// Insert a root, drain any buffered attributions, and emit `Release` since
    /// the root is now locally available.
    ///
    /// L-5: when a repost wrapper already keyed this id (`supersedes_target`
    /// set, empty/placeholder card), the arriving target hydrates the card body
    /// **without losing the repost provenance** — the existing
    /// `supersedes_target` is preserved so the renderer still shows the
    /// "reposted by" banner. A plain (non-reposted) root just inserts.
    fn ingest_root(&self, event: &KernelEvent) {
        let Ok(mut st) = self.state.lock() else { return };
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
        // L-5 backward path: a repost wrapper keyed this id first. Re-fetch the
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
        st.roots.insert(
            event.id.clone(),
            RootSlot {
                card,
                created_at,
                author_pubkey: event.author.clone(),
                supersedes_target,
                wrapper_event_id,
            },
        );
        Self::drain_pending_into(&mut st, &event.id);
        self.emit_release_for(&mut st, &event.id);
    }

    /// Repost-shaped event (`supersedes == Some(target)`): the target becomes
    /// the surfaced root. Insert the wrapper card keyed by the target id; claim
    /// the target if absent (L-1); hydrate from the pair if already local (L-5
    /// forward direction).
    fn ingest_repost(&self, wrapper: &KernelEvent, target: EventId) {
        let Ok(mut st) = self.state.lock() else { return };
        let target_event = (self.caps.event_lookup)(&target);
        let card = (self.caps.card_builder)(wrapper, target_event.as_ref());
        st.roots.insert(
            target.clone(),
            RootSlot {
                card,
                created_at: wrapper.created_at,
                author_pubkey: wrapper.author.clone(),
                supersedes_target: Some(target.clone()),
                wrapper_event_id: Some(wrapper.id.clone()),
            },
        );
        Self::drain_pending_into(&mut st, &target);
        if target_event.is_none() {
            // L-1 / L-5: target not local → claim it.
            let pointer = ThreadPointer::Event {
                id: target.clone(),
                relay: None,
                kind: None,
            };
            self.emit_claim(&mut st, &target, pointer, Vec::new());
        } else {
            self.emit_release_for(&mut st, &target);
        }
    }

    /// Reply from a followed author. Resolve the referenced root, re-key past a
    /// repost wrapper if applicable (L-2), record the attribution against the
    /// root (or buffer it), and claim the root if not locally held.
    fn ingest_reply(&self, event: &KernelEvent, pointer: ThreadPointer) {
        // Prefer the explicit root pointer; fall back to the parent pointer.
        let resolved = self.caps.resolver.root(event).unwrap_or(pointer);

        // Only Event pointers are hydratable. Address is post-v1; External is
        // terminal — both attach against a surrogate primary id with no claim.
        let (primary_id, claim_pointer): (EventId, Option<ThreadPointer>) = match &resolved {
            ThreadPointer::Event { id, .. } => {
                // L-2: the reply targets a repost wrapper that is locally known
                // and supersedes a different id → re-key to that target.
                let rekeyed = (self.caps.event_lookup)(id)
                    .and_then(|parent| self.caps.resolver.supersedes(&parent));
                match rekeyed {
                    Some(target) => (target, Some(rekey_pointer(&resolved, id))),
                    None => (id.clone(), Some(resolved.clone())),
                }
            }
            // Address roots are claimable (post-v1 path) but carry the coord as
            // the surrogate primary id so attribution still attaches meanwhile.
            ThreadPointer::Address { coord, .. } => (coord.clone(), Some(resolved.clone())),
            // External is terminal: never claimed; attaches to a URI surrogate.
            ThreadPointer::External { uri } => (external_surrogate(uri), None),
        };

        let Some(attribution) =
            A::from_reply(event, self.caps.follow.as_ref(), &|pk| self.profile_for(pk))
        else {
            return;
        };

        let Ok(mut st) = self.state.lock() else { return };
        if st.roots.contains_key(&primary_id) {
            Self::record_attribution(&mut st.attributions, &primary_id, attribution);
        } else {
            Self::record_attribution(&mut st.pending_attributions, &primary_id, attribution);
            if let Some(pointer) = claim_pointer {
                let hints = reply_provenance_hints(event);
                self.emit_claim(&mut st, &primary_id, pointer, hints);
            }
        }
    }

    /// Drain buffered attributions for `root_id` into the live map. Called once
    /// the root is locally held.
    ///
    /// Does NOT touch `pending_pointers` — `emit_release_for` is the single
    /// owner of pointer removal so it can still read the pointer to build the
    /// `Release`. (An early `remove` here was an ordering bug: drain-then-
    /// release found nothing to release.)
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

/// Re-key a resolved `Event` pointer onto a (possibly different) id while
/// keeping the original relay/kind hint TLVs. Non-`Event` pointers pass through.
fn rekey_pointer(original: &ThreadPointer, id: &str) -> ThreadPointer {
    match original {
        ThreadPointer::Event { relay, kind, .. } => ThreadPointer::Event {
            id: id.to_string(),
            relay: relay.clone(),
            kind: *kind,
        },
        other => other.clone(),
    }
}

/// Stable surrogate id for an external (non-Nostr) root reference. Lets
/// attribution attach even though the engine never hydrates it. The
/// `external:` prefix guarantees it never collides with a 64-hex event id.
fn external_surrogate(uri: &str) -> EventId {
    format!("external:{uri}")
}

/// Provenance relay hints for the root claim seeded from the referencing event.
///
/// The reply's provenance relay lives in the kernel, not in the `KernelEvent`
/// the engine receives (V-64). Per design §3-B step 4, the cleaner shape is for
/// the wiring/kernel side to resolve the relay from the reply id, so the engine
/// emits no hint here — keeping the claim identical to every other
/// `claim_event` caller. The wiring layer may enrich the claim if it chooses.
fn reply_provenance_hints(_event: &KernelEvent) -> Vec<RelayHint> {
    Vec::new()
}
