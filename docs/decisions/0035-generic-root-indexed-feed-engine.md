# ADR-0035 - Generic root-indexed feed engine in `nmp-feed`

Status: accepted

Date: 2026-05-28

## Context

The home feed product model is changing from "threaded notes (replies + roots)
over the follow-set" to **"thread roots only, with follows' replies as
attribution metadata on their root"** (former tracker V-80; full design in
[`docs/perf/op-centric-feed-architecture.md`](../perf/op-centric-feed-architecture.md)).
A followed user's reply to a *non-followed* root must surface that root with an
"↳ Alice replied" badge. Reply rows never stand alone.

The mechanics of this model — index by root, buffer attributions for
not-yet-local roots, and handle repost wrappers/placeholders — are
protocol-agnostic. The feed engine does **not** hydrate the missing root, fetch
profiles, count replies, render previews, or acquire any other secondary data.
Mounted UI components or sibling modules own those dependencies when they need
them. The protocol-specific parts are: what counts as a reply/root/repost edge
(NIP-10 markers, NIP-22 markers, …), what an attribution payload looks like,
and how a render card is built from a local event/target pair. Per D0/D7 those
belong in protocol-instance crates, not in the substrate-generic feed crate.

This rung (3 of the 7-rung V-80 ladder) delivers the **engine only**, with
synthetic tests. No protocol instance, no Chirp wiring (rungs 4–7). Master
behavior is unchanged.

## Decision

Add a generic engine `RootIndexedFeed<R, A, C>` to `nmp-feed`, parameterized
over:

- `R: nmp_threading::ParentResolver` — resolves parent / root / supersedes
  edges from a `KernelEvent`. No kind numbers in the engine.
- `A: AttributionPayload` — the per-root attribution metadata. It carries raw
  attribution data only; there is no associated profile type.
- `C: Serialize + Clone` — the render card produced by a host-supplied builder.

### Capabilities are closures, not traits (D7)

The engine takes its capabilities as construction-time closures, never as
trait objects it must name a producer for:

- `FollowPredicate = Arc<dyn Fn(&str) -> bool + Send + Sync>` — follow-set
  membership. **No `FollowSetLookup` trait** (that shape created a
  `nmp-feed → nmp-core → nmp-planner` cycle; see design §3-D). The producer
  lives in `nmp-nip02`; the predicate is wired by the composition root.
- `EventLookup = Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>` —
  local read-cache lookup, needed for repost L-2 / L-5 rebuild. This is not an
  acquisition seam; it may only return already-cached events.
- `CardBuilder<C>` — boxed closure that builds a card from `(root,
  Option<target>)`. The `Option<target>` second arg is what lets L-5 rebuild a
  card once the reposted target arrives.

### Value types

- `RootCard<C, A>` = `{ card: C, attribution: Vec<A> }`. **No `attribution_total`
  field** (user Q1): the `Vec` length IS the count; each render surface decides
  how many to show. Explicit `C: Serialize + Clone` / `A: Serialize + Clone`
  bounds with `#[serde(bound(...))]` (codex M4).
- `RootFeedSnapshot<C, A>` = `{ cards, page, metrics }`, the visible-window
  projection. The engine windows newest-first directly over its bounded `roots`
  map.

### State (all D5-bounded)

`roots`, `attributions` (root_id → reply_id → A), and `pending_attributions`
(buffered for not-yet-local roots). Repost placeholders are keyed by target id
until the target arrives through normal event ingest. The outer maps are
`MAX_PROJECTION_MESSAGES`-bounded; each per-root attribution sub-map is
`MAX_ATTRIBUTION_PER_ROOT` (= 64) bounded. The engine implements
`KernelEventObserver` (ingest) and feed snapshot/window mechanics.

## Secondary Data Boundary

Firm rule (D0 + D11): the feed engine acquires events only through the
`KernelEventObserver` ingest path. It does not emit `claim_event`, observe
`event_claim_released`, call `release_claim_expansion`, translate pointers into
`nostr:` URIs, or otherwise turn a rendered edge into a new acquisition request.

Why: a feed is a bounded indexing and viewport machine. Missing targets,
profiles, ancestors, relation counts, previews, and other secondary data are
dependencies of the component or sibling module that renders or calculates with
them. Putting those claims in the feed would couple the feed primitive to one
protocol's event model and make it an oversized building block.

Consequence: missing roots remain bounded pending feed state until the root
arrives through normal ingest, a perspective reset clears the feed, or D5
eviction removes the pending attribution. E-tag-only reposts may surface a
target-keyed placeholder; target acquisition belongs to a mounted row/component
or another module that explicitly needs the target. The `EventLookup` callback
is a local read-cache lookup for repost rebuilds only; it may return already
cached events but must never acquire.

Profile kind:0 data is also outside this engine. A profile/avatar component
that renders a pubkey owns the profile dependency regardless of whether that
pubkey came from a feed card, a reply attribution, a mention, or another
surface.

## Doctrine

- **D0**: `nmp-feed` names zero protocol/profile tokens. A CI grep gate
  (`crates/nmp-testing/tests/op_feed_doctrine_lint.rs`) fails the build if any
  `.rs` under `crates/nmp-feed/src/` contains `nipNN`, `marmot`, or
  `ProfileDisplay` (case-insensitive).
- **D5**: every map bounded; the snapshot is visible-window-only; per-root
  attribution sub-maps independently capped. Proven by
  `d5_visible_window_bounds_card_count_and_json` (2,000 roots → 80-card window,
  bounded JSON) and `per_root_submap_evicts_oldest_without_release`.
- **D7**: closure-shaped capabilities; the engine asks, the wiring decides.
- **D8**: observer-driven; no polling.
- **D11**: no new bespoke C-ABI symbol; feed wiring does not acquire secondary
  events.

## Consequences

- The engine ships unwired with 17 synthetic tests; Chirp is untouched and
  master stays green.
- A second protocol instance (the post-v1 `nmp-nip22` kind:1111 comment-tree
  feed) composes with `(R, A, C)` only — zero engine changes.
- **L-5 late-target rebuilds from the `(wrapper, target)` pair.** When a
  repost wrapper keys a target id before the target arrives, the slot records
  `wrapper_event_id`; on target arrival the engine reads the already-cached
  wrapper via `event_lookup` and calls `card_builder(wrapper, Some(target))`, so a renderer
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
