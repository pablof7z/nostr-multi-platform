//! `register_op_feed` — note-feed instance wiring of the generic
//! `RootIndexedFeed` engine.
//!
//! Binds the three generic parameters of the engine to NIP-10:
//!
//! * `R = NoteFeedResolver` — parent/root edges from NIP-10 markers plus
//!   supersedes edges from NIP-18 repost wrappers.
//! * `A = Nip10ReplyAttribution` — the NIP-10 reply attribution payload
//!   (`super::attribution`).
//! * `C = NoteFeedItem` — the feed-owned row payload.
//!
//! # Why no `&NmpApp` parameter
//!
//! The design doc (`docs/perf/op-centric-feed-architecture.md` §3-A) sketches
//! `register_op_feed(app: &NmpApp, …)`. That is pseudocode, exactly as rung 4
//! documented for `ActiveFollowSet::new(app)`: `NmpApp` lives in `nmp-ffi`,
//! which this crate does not depend on. The substrate-clean realization —
//! mirroring
//! `nmp_nip02::ActiveFollowSet` — is to construct the engine here and hand the
//! caller back the `Arc<OpFeedEngine>`. The composition root (rung 6,
//! `explicit composition`, which *does* depend on `nmp-ffi`) performs the
//! `NmpApp`-level registration:
//!
//! ```ignore
//! let engine = nmp_note_feed::register_op_feed(viewer, predicate, lookup);
//! app.open_observed_projection(ObservedProjection::from_shape(
//!     Arc::clone(&engine) as Arc<dyn ObservedProjectionSink>,
//!     "nmp.feed.home",
//!     0,
//!     feed_shape,
//!     256,
//! ));
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
//! data mount their own event-ref / profile / count dependency at render time.
//!
//! # D-doctrine
//!
//! * **D0** — this crate is a feed composition crate; protocol-specific
//!   bindings live here, while `nmp-core` / `nmp-feed` stay protocol-generic.
//! * **D7** — the follow predicate and event lookup are closures injected by
//!   the composition root. The event lookup is a local read-cache lookup only,
//!   not an acquisition seam.
//! * **D8** — no polling, no blocking.

use std::sync::Arc;

use nmp_core::substrate::{KernelEvent, SuppressionLookup};
use nmp_core::tags::parse_nip10;
use nmp_core::ObservedProjectionSink;
use nmp_feed::{
    admit_all_roots, CardBuilder, EventGate, EventLookup, FollowPredicate, RootIndexedFeed,
};
use nmp_threading::{ParentResolver, ThreadPointer};

use super::attribution::Nip10ReplyAttribution;
use crate::NoteFeedItem;

pub type Pubkey = String;

pub struct NoteFeedResolver;

impl ParentResolver for NoteFeedResolver {
    fn parent(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        if event.kind == nmp_nip18::KIND_REPOST {
            return None;
        }
        let refs = parse_nip10(&event.tags);
        refs.reply.map(|r| ThreadPointer::Event {
            id: r.id,
            relay: r.relay,
            kind: None,
        })
    }

    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        if event.kind == nmp_nip18::KIND_REPOST {
            return None;
        }
        let refs = parse_nip10(&event.tags);
        refs.root.map(|r| ThreadPointer::Event {
            id: r.id,
            relay: r.relay,
            kind: None,
        })
    }

    fn parent_author(&self, event: &KernelEvent) -> Option<String> {
        if event.kind == nmp_nip18::KIND_REPOST {
            return None;
        }
        parse_nip10(&event.tags)
            .mentioned_pubkeys
            .into_iter()
            .next()
    }

    fn supersedes(&self, event: &KernelEvent) -> Option<String> {
        if event.kind != nmp_nip18::KIND_REPOST {
            return None;
        }
        nmp_nip18::try_from_kernel_event(event).and_then(|record| record.target_event_id)
    }
}

/// The note-feed instance of the generic feed engine.
pub type OpFeedEngine = RootIndexedFeed<NoteFeedResolver, Nip10ReplyAttribution, NoteFeedItem>;

/// Snapshot / feed-registry key for the OP-centric home feed. Matches the key
/// Chirp's `ModularTimelineProjection` registers today; the swap to this engine
/// is rung 7, so this rung leaves the key registered ONLY inside tests.
pub const OP_FEED_SNAPSHOT_KEY: &str = "nmp.feed.home";

/// Construct (but do not register) the NIP-10 OP-feed engine.
///
/// Returns the `Arc<OpFeedEngine>`. The composition root registers it as a
/// `ObservedProjectionSink` (ingest) and a `FeedController` under
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
    // The home feed admits EVERY root the acquisition delivers (the followed
    // authors' timeline is gated by the acquisition filter, not an engine-level
    // admission predicate). The `follow_predicate` still gates reply
    // attribution.
    register_op_feed_with_admission(viewer, follow_predicate, admit_all_roots(), event_lookup)
}

/// Construct the OP-feed engine with an explicit ROOT-admission predicate.
///
/// Like [`register_op_feed`] but gates which roots ENTER the feed by the
/// compiled perspective `root_admission` (#1740 step 3) instead of admitting all
/// roots. Scoped feed sessions (`ContactList` / `ListMembers` / `Wot` /
/// `Difference` / set algebra) build through here so a non-member author never
/// renders as a root. The `follow_predicate` still gates only reply attribution.
#[must_use]
pub fn register_op_feed_with_admission(
    viewer: Pubkey,
    follow_predicate: FollowPredicate,
    root_admission: nmp_feed::RootAdmission,
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
        event.kind == nmp_nip01::KIND_SHORT_TEXT_NOTE || event.kind == nmp_nip18::KIND_REPOST
    });

    let card_builder: CardBuilder<NoteFeedItem> =
        Box::new(|root: &KernelEvent, target: Option<&KernelEvent>| {
            NoteFeedItem::from_event_for_op_feed(root, target)
        });

    Arc::new(RootIndexedFeed::new(
        NoteFeedResolver,
        follow_predicate,
        root_admission,
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
    fn apply_delete(&self, record: &nmp_nip18::DeleteRecord) {
        // NIP-01 short-text notes are not addressable, so only `e`-tag
        // (event-id) targets resolve to a row; an `a`-tag coordinate has no
        // op-feed root and is a no-op. A delete only removes a row the same
        // author published (NIP-09). An `e` id can name two distinct shapes:
        for target_id in &record.event_targets {
            // (a) the note/root itself — keyed by its own id, author = note author;
            self.engine
                .remove_root_if(target_id, |card| card.author_pubkey == record.author);
            // (b) a kind:6 repost *wrapper* that surfaced a target — keyed by the
            //     target id, so a delete naming the wrapper id must match on the
            //     wrapper id and validate against the reposter (wrapper author).
            //     Without this, deleting your repost leaves the reposted row.
            self.engine.remove_root_by_wrapper_if(target_id, |card| {
                card.reposted_by
                    .as_ref()
                    .is_some_and(|repost| repost.author_pubkey == record.author)
            });
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

impl ObservedProjectionSink for OpFeedObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if let Some(record) = nmp_nip18::DeleteRecord::try_from_kernel_event(event) {
            self.apply_delete(&record);
            return;
        }
        if event.kind == nmp_nip01::KIND_SHORT_TEXT_NOTE {
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
