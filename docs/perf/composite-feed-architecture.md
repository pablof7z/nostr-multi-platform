# Composite-Feed Architecture

> **Status:** Shipped (#3082/#3086). Design record for the composite multi-lane
> feed engine. Supersedes
> [`docs/retired/op-centric-feed-architecture.md`](../retired/op-centric-feed-architecture.md)
> (the demolished `RootIndexedFeed`/`NoteFeedItem`/`AttributionPayload`
> reply-rollup design). See also
> [ADR-0076](../decisions/0076-app-facing-feed-helpers.md),
> [ADR-0070](../decisions/0070-typed-read-sessions.md), and
> [`docs/architecture/crate-boundaries.md`](../architecture/crate-boundaries.md) §8.

---

## 1. Executive summary

The feed layer is composable mechanics only; render policy is app-owned. A
**composite feed** is an additive set of **lanes** over one `FlatFeed`-style
engine (`crates/nmp-feed/src/flat.rs`). Today's single-scope `FeedParams` feed
(`crates/nmp-feed/src/params.rs`) is the degenerate one-lane case — nothing
about it changed.

The redesign is exactly **two additive engine changes**, not a new engine:

1. **Expand arity.** `FlatFeedItemBuilder<C>` is `Fn(&KernelEvent) -> Vec<FlatFeedItem<C>>`
   (was `Option`). One source event can fan into zero, one, or many rows — the
   mechanism a curated-list fan-out (one kind:30001/30003 list → many member
   rows) needs.
2. **Delivery-tagged typed ref vector.** A row (`FeedRow`,
   `crates/nmp-feed/src/feed_row.rs`) carries `refs: Vec<TypedRef>`
   (`crates/nmp-feed/src/typed_ref.rs`). Each `TypedRef` names a target
   (`TypedRefTarget::EventId` or `TypedRefTarget::Address{kind,pubkey,d}`) and a
   `DeliveryMode`:
   - `RenderOnly` — declared into the `refs.event`/embed render channel only
     (the existing D7 lane, `resolve_ref`). The feed sink never receives this
     target as a delivered row.
   - `Delivered` — the target's key is folded into the *same* feed session's
     own acquisition/`live_shapes`, so it re-enters `on_kernel_event` as a real
     delivered event carrying its true `created_at` and its own provenance
     contribution. At most one `Delivered` ref per row
     (`crate::typed_ref::merge_refs`).

This generalizes the earlier `pointer_target_hydration` mechanism into the
declared-ref lane and deletes the disjoint by-id store-peek path (#3083's
cache-luck bug class). The feed **declares** refs; it never calls
`resolve_ref` and never peeks the store by id.

## 2. What was deleted

The baked note/reply engine is gone, not renamed:

- `RootIndexedFeed<R, A>` engine and the `FeedShape::RootIndexed` variant
  (`FeedShape` now has one variant, `Flat` —
  `crates/nmp-feed/src/params.rs`).
- `NoteFeedItem` row, `NoteFeedResolver`, `Nip10ReplyAttribution`,
  `AttributionPayload`, `RootCard.attribution`, `pending_attributions`.
- Reply-rollup ("so-and-so + N replied") as a *feed shape*. A reply digest, if
  an app wants one, is a **concept-owned read** grouping delivered rows — never
  a feed shape.

`nmp-note-feed` is DELETED (#3092, follow-up to #3082/#3086): the single-lane
`FeedParams` path (the ordinary follows timeline) was the last consumer of its
knobs, and it now compiles onto this SAME composite lane-mapping engine
instead — `nmp-feed-session`'s `compile_default_lanes` builds a fixed
`feed.authored` + `nip18.target`-style (`nmp_nip18::nip18_target_render_only_mapping`)
lane pair over the app's declared primary/derived-repost kinds. See
[`docs/architecture/crate-boundaries.md`](../architecture/crate-boundaries.md) §8.

## 3. The composite surface

`crates/nmp-feed/src/composite.rs`:

```rust
pub struct CompositeFeedParams {
    pub key: ProjectionKey,
    pub lanes: Vec<FeedLane>,
    pub render_target_kinds: Vec<u32>,
    pub sort: SortPolicy,
    pub window: FeedWindowPolicy,
    pub item_projection: FeedItemProjection,
}

pub struct FeedLane {
    pub source: FeedScope,
    pub match_kinds: Vec<u32>,
    pub match_tags: BTreeMap<TagKey, BTreeSet<String>>,
    pub mapping: LaneMappingId,
}

pub enum SortPolicy {
    ByInteractionTime,   // newest interaction bumps the row (repost/comment "bump")
    ByTargetCreatedAt,   // sorts by the DELIVERED target's own created_at once known
}
```

- A lane names WHERE events come from (`source: FeedScope`), WHICH events it
  claims (`match_kinds`/`match_tags`), and HOW a claimed event becomes a row
  (`mapping: LaneMappingId`).
- The default lane is `FeedLane::direct(source, kinds)` — renders the source
  directly via `nmp-feed`'s own kind-blind identity mapping
  (`DIRECT_MAPPING_ID = "feed.authored"`). "Render the source" holds by
  construction; target-rendering is a per-lane opt-in, never a baked model.
- `render_target_kinds` is distinct from any lane's `match_kinds`: it names the
  kinds a `Delivered` ref's *target* is allowed to admit as, while a lane's
  `match_kinds` names the *acquired* wrapper/pointer kind.
- The row (`FeedRow`) is `{ canonical_row_id, source_id, author_pubkey, kind,
  created_at, content, tags, relay_provenance, refs: Vec<TypedRef>, context:
  Vec<FeedRowContext> }`. `canonical_row_id` is opaque to the engine — an event
  id, an address coordinate (`kind:pubkey:d`), or a group-scoped
  `coord@group` string computed by the app/protocol mapping. The engine only
  does map lookups on it.
- `FeedRowContext` (`crates/nmp-feed/src/feed_row.rs`) is a provenance SET
  accumulated across every lane contributing to a row: `Authored`,
  `RepostedBy { author_pubkey, note_created_at }`,
  `CommentedBy { author_pubkey, comment_event_id, comment_created_at }`,
  `Group { relay, id }`. There is intentionally no `Reply` variant — reply
  rollup was deleted, not re-homed.

## 4. Boundary: "closures behind ids"

Mappings/extractors cross FFI as **opaque registered ids**
(`LaneMappingId(String)`), the same discipline as `CustomAdmissionId`. Their
closures (`LaneMapping = Arc<dyn Fn(&KernelEvent) -> Vec<MappedRow> + Send +
Sync>`) are constructed in Rust **at the composition root** (ADR-0069) and
never cross FFI.

`LaneMappingRegistry` (`crates/nmp-feed/src/lane_mapping.rs`) lives in
`nmp-feed`, not the higher `nmp-feed-session` compiler layer, so protocol
crates below `nmp-feed-session` in the dependency graph can register into it
without a cycle:

- `nmp-feed` pre-installs the kind-blind identity mapping (`feed.authored`).
- `nmp-nip18` registers `nip18.target` (`NIP18_TARGET_MAPPING_ID`,
  `crates/nmp-nip18/src/lane_mapping.rs`): a repost wrapper → a `Delivered` ref
  to its target, `RepostedBy` provenance.
- `nmp-nip22` registers `nip22.root` (`NIP22_ROOT_MAPPING_ID`,
  `crates/nmp-nip22/src/lane_mapping.rs`): a kind:1111 comment → a `Delivered`
  ref to its root, `CommentedBy` provenance.

Registration is register-once (immutable) — the same fail-open-drift
protection `CustomFeedPolicyRegistry` uses. The engine never learns a kind;
protocol crates own extraction, `nmp-feed` owns only the machinery.

`crates/nmp-native-runtime/src/composite_feed.rs` builds the process-shared
registry at `NmpApp` construction and exposes `NmpApp::open_composite_feed` as
the composition-root entry point, mirroring `NmpApp::open_feed`'s lifecycle
(compile → record teardown in the session registry → return a `FeedHandle`).

## 5. Proven scenarios

`crates/nmp-feed-session/src/composite_compiler_tests.rs` is the driving
example: a composite feed of kind:30023 articles via three lanes — authored
(Direct, address-coordinate keyed), commented (`nip22.root`), reposted
(`nip18.target`) — dedupes to **one row per article**, provenance accumulating
`{Authored, CommentedBy, RepostedBy}`, with the article's real `created_at`
available once delivered under `SortPolicy::ByTargetCreatedAt`. A second test
proves the `Delivered`-ref mechanism admits the article even when the direct
lane's own author-admission would reject it.

Other scenarios the design covers without a baked engine change: a kind:20
picture feed, a kind:1 home feed (a repost collapsing onto its note — the
exact behavior the old `RootIndexed` shape baked, rebuilt generically),
notifications (activity rows, no dedupe), and moderation queues (the row is
the action). App policy the engine deliberately does **not** bake:
render-activity-vs-target, dedupe yes/no, weighted/WoT sort (only admissible
when the sort key is a pure function of delivered data), one-row-vs-many.
Unbounded nested traversal (repost→comment→root) is excluded from the engine —
it is bounded, app/concept-owned work on the refs lane.

## 6. Doctrine compliance

| Doctrine | Status | Notes |
|---|---|---|
| D0 | Yes | `nmp-feed` names no protocol kind; lane mappings are opaque ids resolved at the composition root. |
| D5 | Yes | `merge_refs` bounds refs to at most one `Delivered` target; rows stay visible-window-only. |
| D7 | Yes | `LaneMapping`/admission/order are `Arc<dyn Fn(...)>` closures, never FFI-crossing. |
| D8 | Yes | Mapping closures are pure functions of the delivered event; no store peek (forecloses #3083). |

## 7. What's stubbed (as of #3086)

- `NmpApp::open_composite_feed`
  (`crates/nmp-native-runtime/src/composite_feed.rs`) is implemented,
  registered, and host-tested end to end (`composite_feed_tests.rs`,
  `crates/nmp-testing/tests/composite_feed_driving_example.rs`) — the
  native-runtime composition root is fully wired, not stubbed.
- A UniFFI-support open path exists
  (`open_composite_feed(app, params_json) -> OpenedFeed`,
  `crates/nmp-uniffi-support/src/composite_sessions.rs`, re-exported at
  `lib.rs`) behind the `composite-feed` Cargo feature; it reuses the existing
  `close_feed`/`load_older_feed` lifecycle rather than adding a parallel one.
  Live end-to-end hydration through this surface is no longer blocked — the
  #3088 observed-projection kernel gap is fixed and merged (#3090). The
  remaining genuine gaps are that the `composite-feed` feature is not enabled
  by default in shipping builds, and `nmp gen feed-helpers` codegen does not
  yet emit composite-feed bindings.
- `DeliveredRefDemand` is monotonic within one session's lifetime — a
  declaring row's removal does not yet retract its demand (matches the
  pre-existing `PointerTargetHydration` limitation).
- Mute/delete suppression is not yet re-driven inside the feed (carried over
  unchanged from the pre-composite baseline).

## 8. Related

- [ADR-0076](../decisions/0076-app-facing-feed-helpers.md) — app-facing feed
  APIs as typed read-session helpers.
- [ADR-0070](../decisions/0070-typed-read-sessions.md) — typed read sessions;
  refs now carry a delivery mode.
- [`docs/architecture/crate-boundaries.md`](../architecture/crate-boundaries.md)
  §8 — `nmp-feed` ownership (`nmp-note-feed` deleted, #3092).
- [`docs/builder-guide/07a-build-a-composite-feed.md`](../builder-guide/07a-build-a-composite-feed.md)
  — the builder-facing walkthrough.
- #3082 (design), #3086 (implementation), #3083 (cache-luck reads — deleted by
  construction here), #3085 (follow-set owner-id teardown).
