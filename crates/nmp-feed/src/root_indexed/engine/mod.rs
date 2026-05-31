//! [`RootIndexedFeed`] — the generic OP-centric home-feed engine.
//!
//! Consumes `KernelEvent`s through the kernel observer fan-out and produces a
//! feed of **thread roots only**, each carrying the raw list of attributions
//! (qualifying references from followed authors). Unknown roots are hydrated
//! by emitting a [`ClaimRequest`] through a construction-time closure sink;
//! the engine never touches the action system or any C-ABI symbol (D7).
//!
//! This crate is substrate-generic: it names no protocol convention. The
//! resolver (`R: ParentResolver`) decides parent/root/supersedes edges; the
//! payload (`A: AttributionPayload`) decides what qualifies as attribution and
//! how a card's author display refreshes; the follow predicate and event
//! lookup are plain closures. A CI grep gate enforces zero NIP/profile tokens.
//!
//! ## V-81 — the release signal is NOT terminal
//!
//! Rung 1's `event_claim_released` ring fires on **Phase-1 EOSE**, which is
//! *not* the final "this root will never arrive" verdict — Phase-2 relay
//! retargeting may still be fetching the root. The design doc §3-D originally
//! said `on_claim_released` should drop `pending_attributions[root]`; **BACKLOG
//! V-81 supersedes that** (it postdates the doc). [`Self::on_event_claim_released`]
//! is therefore a no-op beyond a diagnostic counter: a pending attribution
//! survives a release signal and is dropped ONLY when (a) the root actually
//! arrives (drains pending → attributions) or (b) the bounded map evicts it
//! under D5 capacity pressure. See ADR-0035 for the full rationale. The
//! cleaner long-term fix — moving the ring push to `terminate_claim` in
//! `nmp-core` — is recorded as a rung-1 follow-up in V-81, not implemented
//! here (this rung is `nmp-feed`-only).

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use nmp_core::planner::RelayHint;
use nmp_core::substrate::{
    empty_suppression_lookup, BoundedMessageMap, EventId, KernelEvent, MAX_PROJECTION_MESSAGES,
    SuppressionLookup,
};
use nmp_threading::{pointer::ThreadPointer, ParentResolver};

use crate::root_indexed::attribution::AttributionPayload;
use crate::root_indexed::card::{RootCard, RootFeedSnapshot};
use crate::root_indexed::claim::ClaimRequest;
use crate::{FeedController, FeedCursor, FeedPage, FeedRequest};

/// The per-event ingest state machine lives in a sibling file to keep both
/// under the 500-LOC ceiling; it is a continuation `impl` on `RootIndexedFeed`.
mod ingest;

/// Per-root D5 cap: at most this many attributions per root sub-map. When the
/// sub-map is full the oldest reply (by [`AttributionPayload::reply_created_at`])
/// is evicted. Independent of the global [`MAX_PROJECTION_MESSAGES`] outer cap.
pub const MAX_ATTRIBUTION_PER_ROOT: usize = 64;

/// Predicate the engine consults to decide whether a referencing author's
/// reference qualifies as attribution (the follow-set membership test, wired by
/// the composition root — never a trait, never planner-coupled; D7).
pub type FollowPredicate = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Read-cache lookup the engine uses for repost L-2 / L-5 rebuild.
pub type EventLookup = Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>;

/// Sink the engine pushes [`ClaimRequest`]s through. The wiring layer turns
/// these into host hydration calls; the engine stays free of the action system.
pub type ClaimSink = Arc<dyn Fn(ClaimRequest) + Send + Sync>;

/// Detect a profile event and extract `(author_pubkey, profile)`; `None` for
/// non-profile events. Lets the engine fan profiles out without naming a kind.
pub type ProfileDetector<A> =
    Box<dyn Fn(&KernelEvent) -> Option<(String, <A as AttributionPayload>::Profile)> + Send + Sync>;

/// Gate predicate: `true` for feed-eligible event kinds (roots or attributions).
/// Events that fail the gate are dropped at the observer entry point before any
/// state is touched. Caller-supplied so the engine stays kind-agnostic (D0).
pub type EventGate = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

/// Build a render card from a root event, plus the supersedes-target event when
/// present (L-5 hydration rebuilds with both).
pub type CardBuilder<C> = Box<dyn Fn(&KernelEvent, Option<&KernelEvent>) -> C + Send + Sync>;

/// A locally-held root and its render card + bookkeeping for repost rebuild.
struct RootSlot<C> {
    card: C,
    created_at: u64,
    /// Hex pubkey of the event's author. Used by the suppression filter in
    /// `snapshot()` to hide muted authors without storing a pubkey on the card.
    author_pubkey: String,
    /// When this root is a repost-style wrapper, the id it supersedes. Used to
    /// hydrate the card while preserving repost provenance once the wrapped
    /// target arrives (L-5).
    supersedes_target: Option<EventId>,
    /// The kind:6 repost wrapper event id, when this slot was seeded by a
    /// repost. On L-5 backward hydration (target arrives after the wrapper),
    /// the engine re-fetches the wrapper via `event_lookup` and rebuilds the
    /// card from the `(wrapper, target)` pair so a renderer can still show the
    /// "reposted by" provenance. `None` for plain roots.
    wrapper_event_id: Option<EventId>,
}

/// Closure capability bundle, all shared/owned closures. Held outside the
/// `Mutex` (they are immutable after construction) so the hot observer path
/// does not contend on capability access.
struct Capabilities<R, A: AttributionPayload, C> {
    resolver: R,
    follow: FollowPredicate,
    event_gate: EventGate,
    event_lookup: EventLookup,
    claim_sink: ClaimSink,
    profile_detector: ProfileDetector<A>,
    card_builder: CardBuilder<C>,
    consumer_id: String,
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
    /// referenced root id. Survives a release signal (V-81).
    pending_attributions: BoundedMessageMap<EventId, BoundedMessageMap<EventId, A>>,
    /// Pointer the engine emitted a `Claim` for, keyed by primary id, so a
    /// `Release` can carry the correct pointer shape.
    pending_pointers: BoundedMessageMap<EventId, ThreadPointer>,
    /// Author pubkey → most recent profile, for in-place display refresh.
    profiles: BoundedMessageMap<String, A::Profile>,
}

impl<A: AttributionPayload, C> EngineState<A, C> {
    fn new() -> Self {
        Self {
            roots: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
            attributions: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
            pending_attributions: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
            pending_pointers: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
            profiles: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
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
    caps: Capabilities<R, A, C>,
    state: Mutex<EngineState<A, C>>,
    /// Mute-list (or other suppression) backend. Consulted in `snapshot()` to
    /// filter out roots whose author is on the active account's suppression set.
    /// Defaults to [`EmptySuppressionLookup`] (pass-through) so the engine works
    /// correctly before any mute-list projection is wired in.
    suppression: Mutex<Arc<dyn SuppressionLookup>>,
    /// Diagnostic counter of release signals seen. Per V-81 these do NOT evict
    /// pending attributions; the counter exists so a consumer (and the V-81
    /// test) can observe "release seen, pending intact".
    released_signals_seen: AtomicU64,
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
    /// * `claim_sink` — receives every [`ClaimRequest`]; the wiring layer turns
    ///   these into host hydration calls.
    /// * `profile_detector` — extracts `(author, profile)` from a profile
    ///   event, `None` otherwise.
    /// * `card_builder` — `(root_event, Option<target_event>) -> C`.
    /// * `consumer_id` — refcount/match key stamped on every emitted claim.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resolver: R,
        follow: FollowPredicate,
        event_gate: EventGate,
        event_lookup: EventLookup,
        claim_sink: ClaimSink,
        profile_detector: ProfileDetector<A>,
        card_builder: CardBuilder<C>,
        consumer_id: impl Into<String>,
    ) -> Self {
        Self {
            caps: Capabilities {
                resolver,
                follow,
                event_gate,
                event_lookup,
                claim_sink,
                profile_detector,
                card_builder,
                consumer_id: consumer_id.into(),
            },
            state: Mutex::new(EngineState::new()),
            suppression: Mutex::new(empty_suppression_lookup()),
            released_signals_seen: AtomicU64::new(0),
        }
    }

    /// Swap in a live suppression backend (e.g. a `MuteListProjection`).
    ///
    /// Called once at composition time after the mute-list observer is
    /// registered. The new lookup takes effect on the next `snapshot()` call.
    /// Fails silently on a poisoned mutex (D6).
    pub fn set_suppression(&self, lookup: Arc<dyn SuppressionLookup>) {
        if let Ok(mut guard) = self.suppression.lock() {
            *guard = lookup;
        }
    }

    /// Fan a freshly-arrived profile into every attribution (live + pending)
    /// for its author, and cache it for future `from_reply` calls.
    fn apply_profile(&self, pubkey: &str, profile: A::Profile) {
        let Ok(mut st) = self.state.lock() else { return };
        st.profiles.insert(pubkey.to_string(), profile.clone());
        refresh_author(&mut st.attributions, pubkey, &profile);
        refresh_author(&mut st.pending_attributions, pubkey, &profile);
    }

    fn profile_for(&self, pubkey: &str) -> Option<A::Profile> {
        self.state
            .lock()
            .ok()
            .and_then(|st| st.profiles.get(pubkey).cloned())
    }

    fn emit_claim(
        &self,
        st: &mut EngineState<A, C>,
        primary_id: &str,
        pointer: ThreadPointer,
        hints: Vec<RelayHint>,
    ) {
        st.pending_pointers
            .insert(primary_id.to_string(), pointer.clone());
        (self.caps.claim_sink)(ClaimRequest::Claim {
            pointer,
            hints,
            consumer_id: self.caps.consumer_id.clone(),
        });
    }

    /// Emit `Release` for a now-resolved primary id if we had a pointer
    /// recorded for it.
    fn emit_release_for(&self, st: &mut EngineState<A, C>, primary_id: &str) {
        if let Some(pointer) = st.pending_pointers.remove(primary_id) {
            (self.caps.claim_sink)(ClaimRequest::Release {
                pointer,
                consumer_id: self.caps.consumer_id.clone(),
            });
        }
    }

    /// Rung-1 `event_claim_released` observer hook.
    ///
    /// **V-81: this is intentionally NOT terminal.** A Phase-1-EOSE release
    /// signal does not mean the root will never arrive (Phase-2 retargeting may
    /// still be running), so the engine MUST NOT drop `pending_attributions`
    /// here. We only bump a diagnostic counter. Pending entries are dropped
    /// solely by root arrival (drain) or D5 capacity eviction. See ADR-0035 +
    /// BACKLOG V-81.
    pub fn on_event_claim_released(&self, _primary_id: &EventId) {
        self.released_signals_seen.fetch_add(1, Ordering::Relaxed);
    }

    /// Number of release signals observed (diagnostic; V-81).
    #[must_use]
    pub fn released_signals_seen(&self) -> u64 {
        self.released_signals_seen.load(Ordering::Relaxed)
    }

    /// Tear down all per-account state on identity change / logout (§3-K). The
    /// wiring layer calls this when `ActiveFollowSet` reports an account
    /// switch. Pending pointers are cleared without emitting Release — the
    /// host clears its own claim refcounts on identity change.
    pub fn reset_for_identity_change(&self) {
        if let Ok(mut st) = self.state.lock() {
            *st = EngineState::new();
        }
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
        // Snapshot the suppression lookup once before iterating — avoids
        // re-locking per card. Fail-open: if the mutex is poisoned, fall back
        // to the empty lookup (suppress nothing) per D6.
        let suppression = self
            .suppression
            .lock()
            .map(|g| Arc::clone(&*g))
            .unwrap_or_else(|_| empty_suppression_lookup());

        // Order roots newest-first by (created_at, id), skipping any root
        // whose author is on the active account's suppression set.
        let mut ordered: Vec<(u64, EventId)> = st
            .roots
            .iter()
            .filter(|(_, slot)| !suppression.is_suppressed_author(&slot.author_pubkey))
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

impl<R, A, C> nmp_core::KernelEventObserver for RootIndexedFeed<R, A, C>
where
    R: ParentResolver,
    A: AttributionPayload + serde::Serialize,
    C: Clone + Send + Sync + serde::Serialize,
{
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ingest(event);
    }
}

impl<R, A, C> FeedController for RootIndexedFeed<R, A, C>
where
    R: ParentResolver,
    A: AttributionPayload + serde::Serialize,
    C: Clone + Send + Sync + serde::Serialize,
{
    fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot(&FeedRequest::default()))
            .unwrap_or(serde_json::Value::Null)
    }

    fn load_older(&self) -> bool {
        // Window growth is driven by the snapshot request limit; the engine
        // holds all roots bounded by D5, so "load older" widens the request
        // limit at the call site. There is no separate paging cursor to
        // advance in the engine itself.
        false
    }
}

/// Refresh display fields for every attribution authored by `pubkey` across a
/// map of per-root sub-maps.
fn refresh_author<A: AttributionPayload>(
    map: &mut BoundedMessageMap<EventId, BoundedMessageMap<EventId, A>>,
    pubkey: &str,
    profile: &A::Profile,
) {
    for sub in map.values_mut() {
        for attribution in sub.values_mut() {
            if attribution.author_pubkey() == pubkey {
                attribution.refresh_for_profile(profile);
            }
        }
    }
}
