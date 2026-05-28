# ADR-0035 - Generic root-indexed feed engine in `nmp-feed`

Status: accepted

Date: 2026-05-28

## Context

The home feed product model is changing from "threaded notes (replies + roots)
over the follow-set" to **"thread roots only, with follows' replies as
attribution metadata on their root"** (BACKLOG V-80; full design in
[`docs/perf/op-centric-feed-architecture.md`](../perf/op-centric-feed-architecture.md)).
A followed user's reply to a *non-followed* root must surface that root with an
"↳ Alice replied" badge. Reply rows never stand alone.

The mechanics of this model — index by root, buffer attributions for
not-yet-local roots, hydrate the missing root, refresh author display, handle
reposts — are protocol-agnostic. The only protocol-specific parts are: what
counts as a reply/root/repost edge (NIP-10 markers, NIP-22 markers, …), what an
attribution payload looks like, and how a profile decodes. Per D0/D7 those
belong in protocol-instance crates, not in the substrate-generic feed crate.

This rung (3 of the 7-rung V-80 ladder) delivers the **engine only**, with
synthetic tests. No protocol instance, no Chirp wiring (rungs 4–7). Master
behavior is unchanged.

## Decision

Add a generic engine `RootIndexedFeed<R, A, C>` to `nmp-feed`, parameterized
over:

- `R: nmp_threading::ParentResolver` — resolves parent / root / supersedes
  edges from a `KernelEvent`. No kind numbers in the engine.
- `A: AttributionPayload` — the per-root attribution metadata, with an
  associated `type Profile`. **`nmp-feed` never names the instance's concrete
  profile type** (the B1 dependency-cycle fix: the engine would otherwise have
  to name the NIP crate's display type, creating a cycle).
- `C: Serialize + Clone` — the render card produced by a host-supplied builder.

### Capabilities are closures, not traits (D7)

The engine takes its capabilities as construction-time closures, never as
trait objects it must name a producer for:

- `FollowPredicate = Arc<dyn Fn(&str) -> bool + Send + Sync>` — follow-set
  membership. **No `FollowSetLookup` trait** (that shape created a
  `nmp-feed → nmp-core → nmp-planner` cycle; see design §3-D). The producer
  lives in `nmp-nip02`; the predicate is wired by the composition root.
- `EventLookup = Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>` —
  read-cache lookup, needed for repost L-2 / L-5 rebuild.
- `ClaimSink = Arc<dyn Fn(ClaimRequest) + Send + Sync>` — the engine emits
  hydration requests through this; it does **not** depend on the action system
  or any C-ABI symbol. The wiring layer translates a `ClaimRequest` into the
  host's existing `claim_event` primitive.
- `ProfileDetector<A>` / `CardBuilder<C>` — boxed closures that extract a
  profile from a profile event and build a card from `(root, Option<target>)`.
  The `Option<target>` second arg is what lets L-5 rebuild a card once the
  reposted target arrives.

### Value types

- `ClaimRequest::{Claim, Release}` carries a `ThreadPointer` (Event / Address /
  External), not a bare id (codex M2), so the wiring layer can encode the right
  NIP-19 URI shape. `External` is terminal — never emitted as a `Claim`.
- `RootCard<C, A>` = `{ card: C, attribution: Vec<A> }`. **No `attribution_total`
  field** (user Q1): the `Vec` length IS the count; each render surface decides
  how many to show. Explicit `C: Serialize + Clone` / `A: Serialize + Clone`
  bounds with `#[serde(bound(...))]` (codex M4).
- `RootFeedSnapshot<C, A>` = `{ cards, page, metrics }`, the visible-window
  projection. The engine windows newest-first directly over its bounded `roots`
  map.

### State (all D5-bounded)

`roots`, `attributions` (root_id → reply_id → A), `pending_attributions`
(buffered for not-yet-local roots), `pending_pointers` (claim pointer by
primary id), `profiles`. The outer maps are `MAX_PROJECTION_MESSAGES`-bounded;
each per-root attribution sub-map is `MAX_ATTRIBUTION_PER_ROOT` (= 64) bounded
and evicts oldest-by-`reply_created_at`. The engine implements
`KernelEventObserver` (ingest) and `FeedController` (snapshot).

## V-81 — the release signal is NOT terminal

Rung 1's `event_claim_released` ring fires on **Phase-1 EOSE**, which is *not*
the final "this root will never arrive" verdict — Phase-2 relay retargeting may
still be fetching the root. The design doc §3-D originally specified
`on_claim_released(primary_id)` should `drop pending_attributions[primary_id]`.
**BACKLOG V-81 (dated after the doc) supersedes that.**

The engine therefore implements **option (a)**: `on_event_claim_released` is a
no-op beyond a diagnostic `AtomicU64` counter. A pending attribution survives a
release signal and is dropped ONLY when (a) the root actually arrives (drains
pending → attributions) or (b) the bounded map evicts it under D5 capacity
pressure. A test (`v81_release_signal_does_not_drop_pending_attribution`) proves
a release signal alone does not drop a pending attribution.

The cleaner long-term fix — **option (b)**, moving the ring push from Phase-1
EOSE to `terminate_claim` in `nmp-core` — is recorded as a rung-1 follow-up in
V-81. It is NOT implemented here: this rung is `nmp-feed`-only and must not
touch `nmp-core`. Making the engine robust to the current Phase-1-EOSE behavior
is correct regardless of whether option (b) lands later.

## Doctrine

- **D0**: `nmp-feed` names zero protocol/profile tokens. A CI grep gate
  (`crates/nmp-testing/tests/op_feed_doctrine_lint.rs`) fails the build if any
  `.rs` under `crates/nmp-feed/src/` contains `nipNN`, `marmot`, or
  `ProfileDisplay` (case-insensitive). `Profile` is an associated type.
- **D5**: every map bounded; the snapshot is visible-window-only; per-root
  attribution sub-maps independently capped. Proven by
  `d5_visible_window_bounds_card_count_and_json` (2,000 roots → 80-card window,
  bounded JSON) and `per_root_submap_evicts_oldest_without_release`.
- **D7**: closure-shaped capabilities; the engine asks, the wiring decides.
- **D8**: observer-driven; no polling.
- **D11**: no new bespoke C-ABI symbol (hydration rides existing `claim_event`).

## Consequences

- The engine ships unwired with 17 synthetic tests; Chirp is untouched and
  master stays green.
- A second protocol instance (the post-v1 `nmp-nip22` kind:1111 comment-tree
  feed) composes with `(R, A, C)` only — zero engine changes.
- The `reply_provenance_hints` helper currently returns an empty `Vec`
  (V-64): the reply's provenance relay lives in the kernel, not in the
  `KernelEvent` the engine sees. The cleanest shape (wiring/kernel resolves the
  relay from the reply id) keeps the claim identical to every other
  `claim_event` caller. If a typed `event_provenance` accessor lands (V-64), the
  wiring layer enriches the claim; the engine surface does not change.
- **Per-root eviction is by insertion order, not `reply_created_at`.** Design
  §3-C describes per-root sub-map eviction as "oldest reply by
  `reply_created_at`". The implementation reuses `BoundedMessageMap`, which
  evicts oldest-by-*insertion*-order. For in-order arrival these coincide; for
  badly out-of-order arrival they diverge (the engine may evict a
  chronologically-newer reply that arrived earlier). This is a deliberate
  simplification — the `reply_created_at` accessor exists on the trait and is
  used for snapshot ordering, just not for eviction. Swapping to a
  timestamp-sorted bounded structure is a future refinement if the divergence
  matters in practice.
- **L-5 backward hydration rebuilds from the `(wrapper, target)` pair.** When a
  repost wrapper keys a target id before the target arrives, the slot records
  `wrapper_event_id`; on target arrival the engine re-fetches the wrapper via
  `event_lookup` and calls `card_builder(wrapper, Some(target))`, so a renderer
  can still produce the "reposted by" provenance after late target arrival.

## Alternatives considered

- **Engine in `nmp-nip01` (design v1).** Rejected: not reusable; bakes NIP-10
  into the feed mechanics.
- **`FollowSetLookup` trait (design v3).** Rejected: created a planner
  dependency cycle (codex B4). Replaced by the closure predicate + composition-
  root expansion (ADR-0036, rung 4/6).
- **`attribution_total` field + N-cap in projection.** Rejected (user Q1): a
  baked-in display decision; the raw `Vec` length is the count, renderers
  choose enumeration.
