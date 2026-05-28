//! `register_op_feed` — the NIP-10 instance wiring of the generic
//! `RootIndexedFeed` engine (V-80 rung 5, Stage 3b).
//!
//! Binds the three generic parameters of the engine to NIP-10:
//!
//! * `R = Nip10Resolver` — parent/root/supersedes edges from NIP-10 markers
//!   and NIP-18 reposts (`crate::meta_timeline::Nip10Resolver`).
//! * `A = Nip10ReplyAttribution` — the NIP-10 reply attribution payload
//!   (`super::attribution`), with `Profile = ProfileDisplay`.
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
//! `nmp-app-template`, which *does* depend on `nmp-ffi`) performs the
//! `NmpApp`-level registration:
//!
//! ```ignore
//! let engine = nmp_nip01::register_op_feed(viewer, predicate, lookup, sink);
//! app.register_event_observer(Arc::clone(&engine) as Arc<dyn KernelEventObserver>);
//! app.register_feed("nmp.feed.home", Arc::clone(&engine) as Arc<dyn FeedController>);
//! ```
//!
//! and supplies the claim sink built from `build_actor_claim_sink` with a
//! dispatcher `Arc::new(move |cmd| app.send_cmd(cmd))`.
//!
//! The snapshot-key `"nmp.feed.home"` is therefore *not* claimed by anything in
//! production in this rung — only `register_op_feed`'s tests register the
//! engine, so there is no runtime conflict with Chirp's existing
//! `ModularTimelineProjection` (rung 7 performs the swap).
//!
//! # Claim sink: `ThreadPointer` → `nostr:` URI
//!
//! The engine emits `ClaimRequest::Claim { pointer, hints, consumer_id }` for a
//! root it does not hold locally. The wiring encodes the pointer as a NIP-19
//! `nostr:` URI and dispatches the existing `ActorCommand::ClaimEvent` (the
//! Rust-level seam behind the `nmp_app_claim_event` C-ABI — `nmp-nip01` calls
//! the actor command directly, never the `extern "C"` symbol, never
//! reimplementing claim logic):
//!
//! * `ThreadPointer::Event { id, relay, kind }` → `nostr:nevent…` (relay TLVs
//!   seeded from `relay` ∪ the `hints` the engine passed; kind TLV when known).
//! * `ThreadPointer::Address { coord = "kind:pubkey:d", relay, kind }` →
//!   `nostr:naddr…` (coord parsed back into the `(kind, pubkey, d)` triple).
//! * `ThreadPointer::External { uri }` → terminal; the engine never emits a
//!   `Claim` for it, so the sink defensively no-ops if one ever arrives.
//!
//! `Release` re-encodes the same pointer and dispatches
//! `ActorCommand::ReleaseEvent` so the kernel refcount stays symmetric.
//!
//! # D-doctrine
//!
//! * **D0** — `nmp-nip01` is a NIP crate; NIP-10 / NIP-19 nouns are fine here.
//!   No NIP token leaks into `nmp-core` / `nmp-feed`.
//! * **D7** — the engine asks (closure sink); the wiring decides (encodes +
//!   dispatches). The follow predicate and event lookup are closures injected
//!   by the composition root.
//! * **D8** — the claim sink does bounded work (one encode, one channel send);
//!   no polling, no blocking.

use std::sync::Arc;

use nmp_core::nip19::{encode_naddr, encode_nevent, NaddrData, NeventData};
use nmp_core::substrate::KernelEvent;
use nmp_core::ActorCommand;
use nmp_feed::{
    CardBuilder, ClaimRequest, ClaimSink, EventLookup, FollowPredicate, ProfileDetector,
    RootIndexedFeed,
};
use nmp_threading::pointer::ThreadPointer;

use super::attribution::Nip10ReplyAttribution;
use crate::meta_timeline::{Nip10Resolver, Pubkey};
use crate::profile_display::profile_from_event;
use crate::timeline_projection::TimelineEventCard;

/// The NIP-10 instance of the generic feed engine: NIP-10 resolver, NIP-10
/// reply attribution, `TimelineEventCard` render card.
pub type OpFeedEngine = RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution, TimelineEventCard>;

/// Snapshot / feed-registry key for the OP-centric home feed. Matches the key
/// Chirp's `ModularTimelineProjection` registers today; the swap to this engine
/// is rung 7, so this rung leaves the key registered ONLY inside tests.
pub const OP_FEED_SNAPSHOT_KEY: &str = "nmp.feed.home";

/// A dispatcher for `ActorCommand`s — the composition root supplies
/// `Arc::new(move |cmd| app.send_cmd(cmd))`; tests supply a recording mock.
pub type ActorCommandDispatch = Arc<dyn Fn(ActorCommand) + Send + Sync>;

/// Construct (but do not register) the NIP-10 OP-feed engine.
///
/// Returns the `Arc<OpFeedEngine>`. The composition root registers it as a
/// `KernelEventObserver` (ingest) and a `FeedController` under
/// [`OP_FEED_SNAPSHOT_KEY`] (output), and forwards `event_claim_released`
/// signals to [`RootIndexedFeed::on_event_claim_released`].
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
/// * `claim_sink` — built via [`build_actor_claim_sink`]; encodes the pointer
///   and dispatches the claim/release actor command.
#[must_use]
pub fn register_op_feed(
    viewer: Pubkey,
    follow_predicate: FollowPredicate,
    event_lookup: EventLookup,
    claim_sink: ClaimSink,
) -> Arc<OpFeedEngine> {
    // `viewer` is carried for parity with `ModularTimelineSpec.viewer` and
    // future per-viewer personalization; the engine has no viewer field today.
    let _ = viewer;

    let profile_detector: ProfileDetector<Nip10ReplyAttribution> =
        Box::new(|event: &KernelEvent| {
            profile_from_event(event).map(|profile| (event.author.clone(), profile))
        });

    let card_builder: CardBuilder<TimelineEventCard> =
        Box::new(|root: &KernelEvent, target: Option<&KernelEvent>| {
            TimelineEventCard::from_event_for_op_feed(root, target)
        });

    Arc::new(RootIndexedFeed::new(
        Nip10Resolver,
        follow_predicate,
        event_lookup,
        claim_sink,
        profile_detector,
        card_builder,
        OP_FEED_SNAPSHOT_KEY,
    ))
}

/// Build the engine's [`ClaimSink`] from an actor-command dispatcher.
///
/// The returned closure encodes each emitted [`ClaimRequest`]'s
/// [`ThreadPointer`] as a `nostr:` URI and dispatches the matching
/// [`ActorCommand`] (`ClaimEvent` / `ReleaseEvent`) — the Rust seam behind the
/// `nmp_app_claim_event` / `nmp_app_release_event` C-ABI. An `External` pointer
/// is terminal (the engine never emits a `Claim` for it); a pointer that fails
/// to encode is silently dropped (D6: a hydration request is best-effort).
#[must_use]
pub fn build_actor_claim_sink(dispatch: ActorCommandDispatch) -> ClaimSink {
    Arc::new(move |request: ClaimRequest| match request {
        ClaimRequest::Claim {
            pointer,
            hints,
            consumer_id,
        } => {
            let hint_relays = hints.into_iter().map(|h| h.url).collect::<Vec<_>>();
            if let Some(uri) = pointer_to_uri(&pointer, &hint_relays) {
                dispatch(ActorCommand::ClaimEvent { uri, consumer_id });
            }
        }
        ClaimRequest::Release {
            pointer,
            consumer_id,
        } => {
            if let Some(uri) = pointer_to_uri(&pointer, &[]) {
                dispatch(ActorCommand::ReleaseEvent { uri, consumer_id });
            }
        }
    })
}

/// Encode a [`ThreadPointer`] as a `nostr:`-prefixed NIP-19 URI.
///
/// * `Event` → `nostr:nevent…` (relay TLVs = `pointer.relay` ∪ `extra_relays`,
///   deduped; kind TLV when known).
/// * `Address` → `nostr:naddr…` (the `coord` is parsed back into the
///   `kind:pubkey:identifier` triple `claim_event` expects).
/// * `External` → `None` (terminal; never claimed).
///
/// Returns `None` on any encode failure (e.g. a malformed coord or non-hex id);
/// the caller treats a `None` as "skip this claim" rather than surfacing an
/// error (D6).
fn pointer_to_uri(pointer: &ThreadPointer, extra_relays: &[String]) -> Option<String> {
    match pointer {
        ThreadPointer::Event { id, relay, kind } => {
            let mut relays: Vec<String> = relay.iter().cloned().collect();
            for r in extra_relays {
                if !relays.contains(r) {
                    relays.push(r.clone());
                }
            }
            let data = NeventData {
                event_id: id.clone(),
                relays,
                author: None,
                kind: *kind,
            };
            encode_nevent(&data)
                .ok()
                .map(|bech| format!("nostr:{bech}"))
        }
        ThreadPointer::Address { coord, relay, .. } => {
            // `coord` is the stable `kind:pubkey:identifier` form (the d-tag may
            // itself contain ':', so split only on the first two — matching
            // `claim_event`'s `event_already_known`).
            let mut parts = coord.splitn(3, ':');
            let kind = parts.next()?.parse::<u32>().ok()?;
            let pubkey = parts.next()?.to_string();
            let identifier = parts.next()?.to_string();
            let mut relays: Vec<String> = relay.iter().cloned().collect();
            for r in extra_relays {
                if !relays.contains(r) {
                    relays.push(r.clone());
                }
            }
            let data = NaddrData {
                identifier,
                pubkey,
                kind,
                relays,
            };
            encode_naddr(&data).ok().map(|bech| format!("nostr:{bech}"))
        }
        // External roots are terminal — the engine never emits a Claim for one.
        ThreadPointer::External { .. } => None,
    }
}
