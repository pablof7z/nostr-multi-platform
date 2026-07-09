# 07a — Build a composite feed

> Status: **LANDED**. Audience: builders + agents.
> The composite-lane compiler, engine, and registry ship at
> `crates/nmp-feed/src/composite.rs`, `crates/nmp-feed/src/lane_mapping.rs`,
> `crates/nmp-feed-session/src/composite_compiler.rs`, and
> `crates/nmp-native-runtime/src/composite_feed.rs` (#3082/#3086), proven end
> to end through a real `NmpApp` and a real relay by
> `crates/nmp-testing/tests/composite_feed_driving_example.rs` (the row-build/
> merge/dedupe logic itself is unit-proven directly in
> `crates/nmp-feed-session/src/composite_compiler_tests.rs`). A UniFFI-support
> open path exists (`open_composite_feed(app, params_json) -> OpenedFeed`,
> `crates/nmp-uniffi-support/src/composite_sessions.rs`) behind the
> `composite-feed` Cargo feature, reusing the existing `close_feed`/
> `load_older_feed` lifecycle; live end-to-end hydration through it is no
> longer blocked (the #3088 kernel gap is fixed and merged, #3090). It is
> **not yet exposed through wasm**, and the `composite-feed` feature is not
> enabled by default in shipping builds, so Swift/Kotlin shells only gain
> this surface once their app opts the feature in; TypeScript/browser shells
> cannot open a composite feed session yet.

## The problem a single-lane feed can't express

[07 — Subscription planner](07-subscription-planner.md) and the ordinary
`app.feeds().open_spec(...)` path ([15 — Codegen and FFI](15-codegen-and-ffi.md))
cover **one scope, one row per admitted event**. That is enough for a plain
following timeline. It is not enough for:

- an article feed that should surface a kind:30023 article once whether the
  active user's follows **authored** it, **commented** on it (NIP-22), or
  **reposted** it (NIP-18) — three different acquired event kinds, one
  canonical row;
- a curated-list feed where one kind:30001/30003 event should expand into
  **many** member rows, not one row per list;
- a quote-card row that needs to render a preview of its target without that
  target ever becoming a delivered row in the feed itself.

None of these are a new engine. They are additive **lanes** over the same
`FlatFeed<FeedRow>` engine every feed already runs on.

## The driving example: a 30023 article composite feed

`crates/nmp-testing/tests/composite_feed_driving_example.rs` proves this
exact scenario end to end, through a real `NmpApp` and a real (fixture)
relay: a composite feed of kind:30023 articles, scoped to the active user's
follows, assembled from three lanes:

| Lane | Acquired kind | Mapping | Provenance added |
|---|---|---|---|
| Authored | 30023 | `Direct` (address-coordinate keyed) | `Authored` |
| Commented | 1111 (NIP-22 comment whose root is the article) | `nip22.root` | `CommentedBy { author_pubkey, comment_event_id, comment_created_at }` |
| Reposted | 16 (NIP-18 repost whose target is the article) | `nip18.target` | `RepostedBy { author_pubkey, note_created_at }` |

All three lanes collapse onto **one row per article**, keyed by the article's
own `kind:pubkey:d` coordinate, with `context` accumulating
`{Authored, CommentedBy, RepostedBy}` as a provenance **set** — never a count,
never a single "who did this" field — and the article's real `created_at`
becoming available once it is delivered. The dedup/provenance-accumulation
logic itself is also unit-proven directly (bypassing `FeedSessionHost`/
acquisition) in `crates/nmp-feed-session/src/composite_compiler_tests.rs`.

## The declaration

```rust
use nmp_feed::{CompositeFeedParams, FeedLane, ProjectionKey, SortPolicy, LaneMappingId, TagKey};

let params = CompositeFeedParams {
    key: ProjectionKey::app("myapp.feed.articles")?,
    lanes: vec![
        // Direct lane: renders the source itself. Zero-config default.
        FeedLane::direct(FeedScope::ActiveUserFollows, vec![30_023]),
        // Comment lane: a kind:1111 comment whose NIP-22 root targets an
        // article folds its target in as a Delivered ref.
        FeedLane {
            source: FeedScope::ActiveUserFollows,
            match_kinds: vec![1111],
            match_tags: [(TagKey("K".into()), ["30023".into()].into())].into(),
            mapping: LaneMappingId("nip22.root".into()),
        },
        // Repost lane: a kind:16 repost whose `k` tag names 30023 folds its
        // target in as a Delivered ref.
        FeedLane {
            source: FeedScope::ActiveUserFollows,
            match_kinds: vec![16],
            match_tags: [(TagKey("k".into()), ["30023".into()].into())].into(),
            mapping: LaneMappingId("nip18.target".into()),
        },
    ],
    render_target_kinds: vec![30_023],
    sort: SortPolicy::ByTargetCreatedAt,
    window: FeedWindowPolicy::bounded(50),
    item_projection: FeedItemProjection::feed_rows(),
};

let handle = app.open_composite_feed(&params)?;
```

`NmpApp::open_composite_feed` (`crates/nmp-native-runtime/src/composite_feed.rs`)
mirrors `open_feed`'s lifecycle exactly: each lane resolves its acquisition
scope through the same step-3 compiler every other feed scope uses, the
resulting teardown recipe is recorded in the same session registry, and the
call returns a `FeedHandle` — the same handle-owned pagination/teardown
contract as a single-lane feed
(`app.feeds().load_older(&handle)` / `app.feeds().close(&handle)`).

## The lane model

A `FeedLane` names three things, never a native closure:

```rust
pub struct FeedLane {
    pub source: FeedScope,                              // WHERE events come from
    pub match_kinds: Vec<u32>,                           // WHICH acquired kinds this lane claims
    pub match_tags: BTreeMap<TagKey, BTreeSet<String>>,  // narrows the claim further (e.g. `K`/`k` scoping)
    pub mapping: LaneMappingId,                          // HOW a claimed event becomes a row (opaque id)
}
```

Multiple lanes may claim the same event (a kind that is both a primary
content kind in one lane and a pointer-wrapper kind in another). The
composite compiler runs **every** matching lane's mapping and the engine's
arity-`Vec` item builder ingests all of them — this is Change 1 of the two
additive engine changes (`FlatFeedItemBuilder<C>` is now
`Fn(&KernelEvent) -> Vec<FlatFeedItem<C>>`, not `Option`).

### Direct vs. a target-mapping lane

`FeedLane::direct(source, kinds)` is the zero-config default lane: it uses
`nmp-feed`'s own kind-blind identity mapping (`DIRECT_MAPPING_ID =
"feed.authored"`, pre-installed by every `LaneMappingRegistry`) —
`canonical_row_id = event.id`, `Authored` provenance, no refs. "Render the
source" holds by construction; every other lane is an explicit opt-in.

A target-mapping lane (`nip18.target`, `nip22.root`, or an app-registered
mapping) instead produces a row whose `canonical_row_id` is the **target's**
key (an event id, or — as in the driving example — an address coordinate),
carrying a `Delivered` ref to that target and a provenance entry describing
*why* this row exists (who reposted it, who commented, when). When the
target itself later arrives (directly, or because the `Delivered` ref folded
it into this session's own acquisition — see below), it collapses onto the
**same** canonical row instead of creating a second one.

## `RenderOnly` vs. `Delivered` refs

Every row's `refs: Vec<TypedRef>` (`crates/nmp-feed/src/typed_ref.rs`) is
delivery-tagged:

- **`RenderOnly`** — declared into the `refs.event`/embed render channel only
  (the existing D7 lane, resolved lazily by `resolve_ref`). The feed sink
  never receives this target as a delivered row. Use this for a quote-card
  preview: the row wants to *show* the quoted note without absorbing it into
  this feed's own acquisition.
- **`Delivered`** — the target's key is folded into **this same session's**
  own `live_shapes` + admission, so the target re-enters `on_kernel_event` as
  a real delivered event, carrying its true `created_at` and contributing its
  own provenance. This is what lets a repost/comment row "become" the real
  article once it arrives, and it is exactly how the driving example's
  `nip18.target`/`nip22.root` lanes work.

At most one `Delivered` ref per row (`TypedRef::merge_refs` drops a second
`Delivered` target rather than admit two "things this row is about"). A lane
mapping never calls `resolve_ref` and never peeks the store by id — that
by-id store peek is the #3083 cache-luck bug class this design forecloses.

`render_target_kinds` on `CompositeFeedParams` is what distinguishes an
acquired wrapper kind from a delivered target's own kind: it is the set of
kinds a `Delivered` ref's target is allowed to admit as (`[30023]` in the
driving example) — distinct from any lane's `match_kinds`, which names the
*acquired* pointer/wrapper kind (`1111`, `16`).

## The two sort policies

```rust
pub enum SortPolicy {
    ByInteractionTime,  // a later repost/comment bumps the row to the top
    ByTargetCreatedAt,   // sorts by the DELIVERED target's own created_at once known
}
```

`ByInteractionTime` is the same "bump on new activity" behavior a following
timeline already has. `ByTargetCreatedAt` is what the driving example uses:
before the article is delivered, a provisional interaction-time proxy holds
the row's position; once delivered, the row sorts by the article's own real
publish time. Either way, `sort_key` **must** be a pure function of delivered
sources — never re-read from a store — so resumable `(created_at, id)`
cursor paging stays correct.

## Registering an app-owned mapping at the composition root

Protocol crates register their own extraction mappings under framework-owned
ids; an app registers its own mappings the same way. Registration happens
**once, at the composition root** (ADR-0069) — never per-screen, never behind
a shell call:

```rust
// crates/nmp-native-runtime/src/composite_feed.rs shows the framework's own
// composition-root registration; an app-owned mapping is added the same way,
// once, at app construction:
fn my_app_lane_mappings() -> LaneMappingRegistry {
    let registry = LaneMappingRegistry::new(); // pre-installs `feed.authored`
    registry.register(
        LaneMappingId(nmp_nip18::NIP18_TARGET_MAPPING_ID.to_string()),
        nmp_nip18::nip18_target_mapping(),
    );
    registry.register(
        LaneMappingId(nmp_nip22::NIP22_ROOT_MAPPING_ID.to_string()),
        nmp_nip22::nip22_root_mapping(),
    );
    registry.register(
        LaneMappingId("myapp.curated_list_member".to_string()),
        Arc::new(|event: &KernelEvent| {
            // Pure function of the delivered event only — no store peek.
            // Arity `Vec`: a curated-list event can expand into many rows.
            expand_list_members(event)
        }),
    );
    registry
}
```

`LaneMappingRegistry::register` is register-once (immutable): a second
registration under the same id is a silent no-op, the same fail-open-drift
protection `CustomFeedPolicyRegistry` uses elsewhere — so an already-open
composite feed can never have its mapping swapped out from under it by a
later registration.

## What this is not

- **Not a new engine.** Every mechanism above (expand → admit →
  group-by(canonical_id) → merge/recompute → sort/window) already shipped in
  `crates/nmp-feed/src/flat.rs`; composite feeds are two additive changes to
  it (arity-`Vec` builder, delivery-tagged refs) plus a lane-mapping registry.
- **Not app policy.** Whether to render the activity or the target, whether
  to dedupe, and any weighted/WoT sort are composition-root decisions, not
  something `nmp-feed` bakes in. The engine never learns a kind or a NIP name
  — lane mappings are the only place kind-specific extraction logic lives,
  and it lives in the protocol crate that owns the kind.
- **Not the reply-rollup shape.** The former `FeedShape::RootIndexed` engine
  ("so-and-so + N replied") is deleted, not rebuilt here. A reply digest, if
  an app wants one, is a **concept-owned read** grouping delivered rows —
  never a feed shape.

See [`docs/perf/composite-feed-architecture.md`](../perf/composite-feed-architecture.md)
for the full design record, and
[`docs/architecture/crate-boundaries.md`](../architecture/crate-boundaries.md)
§8 for crate ownership (`nmp-feed` owns the engine/row/registry; `nmp-nip18`/
`nmp-nip22` own their lane mappings; the single-lane `FeedParams` path
compiles onto this SAME engine via `nmp-feed-session`'s
`compile_default_lanes` — `nmp-note-feed` is deleted, #3092).

See also: [07 — Subscription planner](07-subscription-planner.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[28 — Concept-owned active reads](28-action-triggered-subscriptions.md).
