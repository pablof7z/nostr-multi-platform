//! `register_op_feed` — the NIP-10 instance wiring of the generic
//! `RootIndexedFeed` engine (V-80 rung 5, Stage 3b).
//!
//! Binds the three generic parameters of the engine to NIP-10:
//!
//! * `R = Nip10Resolver` — parent/root/supersedes edges from NIP-10 markers
//!   and NIP-18 reposts (`crate::meta_timeline::Nip10Resolver`).
//! * `A = Nip10ReplyAttribution` — the NIP-10 reply attribution payload
//!   (`super::attribution`).
//! * `C = TimelineEventCard` — the existing render card, built statelessly via
//!   `TimelineEventCard::from_event_for_op_feed`.
//!
//! # Why no `&NmpApp` parameter
//!
//! The design doc (`docs/perf/op-centric-feed-architecture.md` §3-A) sketches
//! `register_op_feed(app: &NmpApp, …)`. That is pseudocode, exactly as rung 4
//! documented for `ActiveFollowSet::new(app)`: `NmpApp` lives in `nmp-ffi`,
//! which `nmp-nip01` depends on only as a *dev*-dependency. A production
//! `&NmpApp` parameter would invert the dependency graph
//! (`nmp-nip01 → nmp-ffi`). The substrate-clean realization — mirroring
//! `nmp_nip02::ActiveFollowSet` — is to construct the engine here and hand the
//! caller back the `Arc<OpFeedEngine>`. The composition root (rung 6,
//! `nmp-defaults`, which *does* depend on `nmp-ffi`) performs the
//! `NmpApp`-level registration:
//!
//! ```ignore
//! let engine = nmp_nip01::register_op_feed(viewer, predicate, lookup);
//! app.register_event_observer(Arc::clone(&engine) as Arc<dyn KernelEventObserver>);
//! app.register_feed("nmp.feed.home", Arc::clone(&engine) as Arc<dyn FeedController>);
//! ```
//!
//! The feed key is a projection/output key only. It is unrelated to the
//! kernel's event-claim refcount mechanism.
//!
//! # Secondary data
//!
//! The OP-feed binding does not claim or fetch roots, targets, profiles, reply
//! counts, media, or previews. Missing roots remain buffered until they arrive
//! through the normal kernel event stream; e-tag-only reposts render as target
//! placeholders until the target arrives. UI components that need secondary
//! data mount their own `claim_event` / profile / count dependency at render
//! time.
//!
//! # D-doctrine
//!
//! * **D0** — `nmp-nip01` is a NIP crate; NIP-10 / NIP-19 nouns are fine here.
//!   No NIP token leaks into `nmp-core` / `nmp-feed`.
//! * **D7** — the follow predicate and event lookup are closures injected by
//!   the composition root. The event lookup is a local read-cache lookup only,
//!   not an acquisition seam.
//! * **D8** — no polling, no blocking.

use std::sync::Arc;

use nmp_core::substrate::{KernelEvent, SuppressionLookup};
use nmp_core::KernelEventObserver;
use nmp_feed::{CardBuilder, EventGate, EventLookup, FollowPredicate, RootIndexedFeed};

use super::attribution::Nip10ReplyAttribution;
use crate::meta_timeline::{Nip10Resolver, Pubkey};
use crate::timeline_projection::TimelineEventCard;

/// The NIP-10 instance of the generic feed engine: NIP-10 resolver, NIP-10
/// reply attribution, `TimelineEventCard` render card.
pub type OpFeedEngine = RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution, TimelineEventCard>;

/// Snapshot / feed-registry key for the OP-centric home feed. Matches the key
/// Chirp's `ModularTimelineProjection` registers today; the swap to this engine
/// is rung 7, so this rung leaves the key registered ONLY inside tests.
pub const OP_FEED_SNAPSHOT_KEY: &str = "nmp.feed.home";
const KIND_EVENT_DELETION: u32 = 5;

/// Construct (but do not register) the NIP-10 OP-feed engine.
///
/// Returns the `Arc<OpFeedEngine>`. The composition root registers it as a
/// `KernelEventObserver` (ingest) and a `FeedController` under
/// [`OP_FEED_SNAPSHOT_KEY`] (output).
///
/// * `viewer` — the active account pubkey (reserved for future
///   personalization; the engine itself is viewer-agnostic, mirroring
///   `ModularTimelineSpec.viewer`).
/// * `follow_predicate` — `true` for pubkeys whose replies/reposts qualify as
///   attribution. Wired from `nmp_nip02::ActiveFollowSet::predicate()`.
/// * `event_lookup` — kernel read-cache lookup keyed by event id, needed for
///   repost L-2 / L-5 rebuild. Note the engine's real signature is
///   `Fn(&EventId) -> Option<KernelEvent>` (the design doc's `Fn(&str) -> …`
///   is the same thing — `EventId` is a `String` alias).
#[must_use]
pub fn register_op_feed(
    viewer: Pubkey,
    follow_predicate: FollowPredicate,
    event_lookup: EventLookup,
) -> Arc<OpFeedEngine> {
    // `viewer` is carried for parity with `ModularTimelineSpec.viewer` and
    // future per-viewer personalization; the engine has no viewer field today.
    let _ = viewer;

    // Gate admits exactly the kinds the engine has a real handler for:
    //   kind:1  → root / reply (NIP-10 short text note)
    //   kind:6  → repost (NIP-18)
    // Every other kind arriving via the observer fan-out (kind:3 contacts,
    // kind:10002 relay lists, …) has NO handler — it only ever reaches the
    // `ingest_root` fall-through and becomes a phantom root card when the relay
    // echoes published events back during account creation. Those kinds are
    // dropped here before any state is touched. The predicate lives at the
    // NIP-10 protocol layer so the generic nmp-feed engine stays kind-agnostic
    // (D0). Profile kind:0 is intentionally excluded: profile components own
    // profile acquisition/rendering independently of the feed.
    let event_gate: EventGate = Arc::new(|event: &KernelEvent| {
        event.kind == crate::kinds::KIND_SHORT_TEXT_NOTE || event.kind == nmp_nip18::KIND_REPOST
    });

    let card_builder: CardBuilder<TimelineEventCard> =
        Box::new(|root: &KernelEvent, target: Option<&KernelEvent>| {
            TimelineEventCard::from_event_for_op_feed(root, target)
        });

    Arc::new(RootIndexedFeed::new(
        Nip10Resolver,
        follow_predicate,
        event_gate,
        event_lookup,
        card_builder,
    ))
}

/// NIP-01 ingest adapter for an [`OpFeedEngine`].
///
/// The generic engine owns root indexing, attribution, ordering, pagination,
/// and reset mechanics. This adapter owns NIP-01/NIP-18 admission policy:
/// short-text roots/replies, kind:6 repost wrappers, NIP-09 deletes that can
/// be validated against the stored card, and caller-supplied suppression.
pub struct OpFeedObserver {
    engine: Arc<OpFeedEngine>,
    event_lookup: EventLookup,
    suppression: Arc<dyn SuppressionLookup>,
}

/// Build the NIP-01 observer adapter for an already-constructed OP feed.
#[must_use]
pub fn op_feed_observer(
    engine: Arc<OpFeedEngine>,
    event_lookup: EventLookup,
    suppression: Arc<dyn SuppressionLookup>,
) -> Arc<OpFeedObserver> {
    Arc::new(OpFeedObserver {
        engine,
        event_lookup,
        suppression,
    })
}

impl OpFeedObserver {
    fn apply_delete(&self, event: &KernelEvent) {
        for target_id in event.tags.iter().filter_map(|tag| event_tag_id(tag)) {
            self.engine
                .remove_root_if(target_id, |card| card.author_pubkey == event.author);
        }
    }

    fn remove_if_suppressed_content(&self, event: &KernelEvent) -> bool {
        if self.suppression.is_suppressed_author(&event.author)
            || self.suppression.is_suppressed_event(&event.id)
        {
            self.engine.remove_root(&event.id);
            return true;
        }
        false
    }

    fn remove_if_suppressed_repost_target(&self, event: &KernelEvent) -> bool {
        let Some(record) = nmp_nip18::try_from_kernel_event(event) else {
            return false;
        };
        let Some(target_id) = record.target_event_id else {
            return false;
        };
        if self.suppression.is_suppressed_event(&target_id) {
            self.engine.remove_root(&target_id);
            return true;
        }
        if record
            .embedded_event
            .as_ref()
            .is_some_and(|target| self.suppression.is_suppressed_author(&target.author))
        {
            self.engine.remove_root(&target_id);
            return true;
        }
        if let Some(target) = (self.event_lookup)(&target_id) {
            if self.suppression.is_suppressed_author(&target.author)
                || self.suppression.is_suppressed_event(&target.id)
            {
                self.engine.remove_root(&target_id);
                return true;
            }
        }
        false
    }
}

impl KernelEventObserver for OpFeedObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind == KIND_EVENT_DELETION {
            self.apply_delete(event);
            return;
        }
        if event.kind == crate::kinds::KIND_SHORT_TEXT_NOTE {
            if !self.remove_if_suppressed_content(event) {
                self.engine.on_kernel_event(event);
            }
            return;
        }
        if event.kind == nmp_nip18::KIND_REPOST {
            if self.suppression.is_suppressed_author(&event.author)
                || self.suppression.is_suppressed_event(&event.id)
                || self.remove_if_suppressed_repost_target(event)
            {
                return;
            }
            self.engine.on_kernel_event(event);
        }
    }
}

fn event_tag_id(tag: &[String]) -> Option<&str> {
    if tag.first().is_some_and(|name| name == "e") {
        tag.get(1).map(String::as_str).filter(|id| !id.is_empty())
    } else {
        None
    }
}
