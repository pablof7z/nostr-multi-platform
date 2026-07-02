# ADR-0076: App-facing feed APIs are typed read-session helpers

## Status

Proposed. This records the public feed API target for #1626. It should move to
Accepted once native, browser, generated helpers, and builder docs teach the
helper shape instead of lower-level compiler wiring.

## Context

ADR-0070 made typed read sessions the app-visible owner of product reads:
acquisition demand, replay, admission, output, status, source changes, and close
belong to one session contract. ADR-0042 demoted raw `open_interest` to
substrate, diagnostic, test, export, or migration machinery. ADR-0036 folded
active-follow reconciliation behind typed sessions. ADR-0035 kept `nmp-feed`
as bounded feed mechanics, not a secondary-data planner. ADR-0053 made
projection declaration output transport, and ADR-0062 made replay-before-live
private session machinery.

The feed implementation now has useful lower-level pieces: `FeedParams`,
`FeedSourceExpr` / `FeedScope`, `FeedHandle`, feed-session compilation,
browser runtime feed opening, UniFFI support helpers that call the default
compiler, and app-owned dynamic projection keys. Issue #1626 remains open
because the normal app-facing shape is still too close to executor wiring:
some current names such as `ChronologicalDesc`, `FeedWindow`, `projection`, and
`CustomPerspectiveId` hide important ownership distinctions.

This ADR does not create a second public read architecture. It specializes
ADR-0070 for feed-shaped helpers.

## Decision

NMP exposes feed-shaped product reads as typed read-session helpers over
ADR-0070.

The normal app-facing feed lifecycle is:

```rust
let handle = app.feeds().open(feed_key, feed_spec)?;
app.feeds().load_older(&handle, page_request)?;
app.feeds().close(&handle);
```

The helper uses the standard NMP feed compiler. App, native, browser, TUI, and
generated helper code do not pass a `FeedCompiler`, observer registrar,
source-effect hook, raw filter JSON, projection registrar, pull controller, or
teardown recipe for normal product feeds.

`FeedParams` remains the canonical serializable descriptor. Ergonomic builders
compile into `FeedParams`; they are not a parallel model. A feed descriptor
contains:

- an app-owned feed key that determines the session's output projection
  identity;
- primary content kinds only;
- a typed source expression;
- an admission policy;
- an order/ranking policy;
- a bounded window policy;
- an item projection/output contract.

Protocol wrapper kinds and maintenance kinds are compiler-derived acquisition,
never app primary input. A Chirp feed declares primary kind `1`; an Olas picture
feed declares primary kind `20`; a long-form article feed declares primary kind
`30023`. Repost wrappers, delete maintenance, and protocol-specific admission
facts are derived below that app boundary by protocol/defaults composition.

Dynamic sources are declared as source expressions and reconciled by the
session. The app does not pass a static copy of the active user's follows,
hosted groups, list members, mute list, WoT set, or other dynamic source family.
Relay-set feeds are expressed through a typed source expression such as a
declared `RelaySetId`; shells do not hand-roll relay routing or mutate relay
membership as feed lifecycle state. Empty dynamic source sets fail closed unless
the descriptor explicitly declares a fallback.

Feed sessions expose stable event, author, address, tag, root, wrapper, and
target pointers as row data when the row schema needs them. They do not own
secondary dependencies. Profiles, event embeds, reply counts, target hydration,
thread hydration, media, badges, reaction summaries, and component-specific
claims are owned by the component, projection, or sibling typed session that
renders or calculates with those refs.

Trellis remains private reconciliation machinery per ADR-0075. No app-facing,
native-facing, web-facing, or builder-guide feed API exposes Trellis types or
asks callers to assemble Trellis graphs.

## Public Shape

The headline API should be intent-level:

```rust
let handle = app.feeds().open(
    FeedKey::app("app.chirp.home")?,
    feed::events()
        .primary_kinds([KIND_NOTE])
        .from(source::active_user().follows())
        .shape(FeedShape::RootIndexed)
        .order(FeedOrder::NewestByFeedPosition)
        .window(FeedWindowPolicy::bounded(100))
        .project(ItemProjection::nip01_op_rows()),
)?;
```

Equivalent product feeds use the same helper:

```rust
let handle = app.feeds().open(
    FeedKey::app("app.olas.following")?,
    feed::events()
        .primary_kinds([KIND_PICTURE])
        .from(source::active_user().follows())
        .shape(FeedShape::Flat)
        .order(FeedOrder::NewestByFeedPosition)
        .window(FeedWindowPolicy::bounded(100))
        .project(ItemProjection::nip68_picture_rows()),
)?;
```

```rust
let handle = app.feeds().open(
    FeedKey::app("app.29er.groups")?,
    feed::events()
        .primary_kinds([KIND_GROUP_EVENT])
        .from(source::active_user().hosted_groups())
        .shape(FeedShape::Flat)
        .order(FeedOrder::NewestByFeedPosition)
        .window(FeedWindowPolicy::bounded(100))
        .project(ItemProjection::group_event_rows()),
)?;
```

The target serializable descriptor behind that helper should converge to this
shape. Current implementation still uses the lower-level field names listed in
the naming ratchet until the migration lands:

```rust
pub struct FeedParams {
    pub key: FeedKey,
    pub primary_kinds: PrimaryKinds,
    pub source: FeedSourceExpr,
    pub admission: FeedAdmission,
    pub order: FeedOrder,
    pub window: FeedWindowPolicy,
    pub item_projection: FeedItemProjection,
}
```

`FeedHandle` remains the close/pagination authority:

```rust
pub struct FeedHandle {
    pub session_id: FeedSessionId,
    pub projection_key: ProjectionKey,
}
```

Close and pagination address the session by handle, not by replaying the key,
filter, source expression, projection key, or params. Key-based `load_older`
survives only as migration/internal plumbing until the helper family owns
pagination by handle.

## Current Naming Ratchet

| Current lower-level name | Public target | Reason |
| --- | --- | --- |
| crate-internal `open_feed_with_compiler(params, compiler)` | `NmpApp::open_feed(params)` now; later `app.feeds().open(feed_key, spec)` | Normal app code must not choose a compiler. |
| explicit compiler seams | Internal test/composition only | Compiler selection is executor wiring. |
| former `PubkeySetExpr` alias | `FeedSourceExpr`, `FeedSource`, or `SourceExpr` | Sources now include relays, tags, referrers, pointer targets, and hosted groups. |
| former `render` field | `shape` | NMP projects row/window shape; hosts render. |
| former `FeedRender::OpCentric` | `FeedShape::RootIndexed` or `ThreadedRootIndex` | The public API should not encode one social-product worldview. |
| `FeedRanking::ChronologicalDesc` | `FeedOrder::NewestByFeedPosition` or `NewestByEventCreatedAt` | Repost/source position and target event time are different contracts. |
| `FeedWindow` with only `initial_limit` | `FeedWindowPolicy` | The contract must have room for page size, budgets, reset/regrow, and exhausted state. |
| `projection` as the only output field | `key` plus `item_projection` | Output identity and row schema are different concepts. |
| `CustomPerspectiveId` reused for source/admission/ranking | phase-specific ids or a policy id with declared capabilities | Source, admission, and ranking are different contracts. |
| `PointerTargets` as casual feed source | explicit target-hydration source or meta-timeline helper | Target hydration must never look like ordinary feed acquisition. |

## Boundaries

`nmp-feed` remains mechanics-only: bounded windows, root-indexed and flat feed
state, pull paging, feed-controller registration, and structural feed-window
wire payloads. It does not own protocol interpretation, profiles, reply counts,
app feed policy, relay/operator defaults, or full-screen dependency planning.

Protocol crates own reusable Nostr meaning: primary event parsing, wrapper
interpretation, address coordinates, delete/replace semantics, reply/root tags,
and concrete item schemas such as note rows or picture rows.

App Rust crates own product feed declarations: which primary content kinds,
which source expression, which admission/order policy, app-owned feed keys, and
which item projection their product renders.

Native, browser, TUI, and desktop shells render typed output and execute
capabilities. They do not parse NIP facts, maintain follow caches, choose relay
policy, expand source sets, or own feed teardown.

## Alternatives Considered

| Alternative | Why rejected |
| --- | --- |
| Keep raw `open_interest` as the feed API | Violates ADR-0042 and ADR-0070; acquisition is not a product read lifecycle. |
| Keep compiler-taking open paths as the normal app API | Leaks compiler/executor wiring and blocks one obvious doorway. |
| Make `FeedSourceExpr` an arbitrary trait or native closure | Crosses FFI badly, breaks the closed data model, and lets policy leak into shells. |
| Make `nmp-feed` own protocol interpretation | Violates ADR-0035; protocol crates own NIP facts and item schemas. |
| Put profiles, embeds, counts, or target hydration in feed declarations | Reintroduces the secondary-data boundary violation ADR-0035 rejected. |
| Make Trellis the public API | Violates ADR-0075; Trellis is private reconciliation substrate. |
| Use projection keys as item projection contracts | Confuses output identity with row schema and hides schema ownership. |

## Fitness Functions / Enforcement

- Product app code opens feeds through `app.feeds().open(...)` or generated
  helpers, not raw `open_interest`.
- Normal app code does not pass a `FeedCompiler`, observer registrar,
  source-effect hook, projection registrar, pull controller, or teardown recipe.
- `FeedParams` rejects wrapper and maintenance kinds as primary input.
- Empty dynamic source sets fail closed and never compile to wildcard relay
  demand.
- Closing a feed uses only `FeedHandle`.
- Feed pagination belongs to the same handle-owned session family.
- Feed output is bounded and typed; raw unbounded event arrays do not cross FFI.
- Feed item projection/schema is declared separately from the app-owned
  projection key.
- Public docs use `FeedSourceExpr`, `FeedSource`, or `SourceExpr` for source
  algebra and do not teach the former pubkey-set name for non-pubkey sources.
- No Trellis types appear in app/native/browser feed APIs.
- Feed declarations do not include profile fetching, reply counts, target
  hydration, thread hydration, media loading, or component-specific claims.
- Builder-guide examples show the north-star helper, not compiler wiring.

## Linked Work

- #1626: app-facing declared-feed north star.
- #1740: typed feed/session implementation work.
- #2611 / #2625: active hosted groups source proof.
- #2626 / ADR-0075: Trellis private reconciliation substrate.
- ADR-0070: typed read sessions own app-visible read lifecycles.
- ADR-0042: generic interests are substrate machinery.
- ADR-0035: generic feed mechanics and secondary-data boundary.
- ADR-0036: active-follow source reconciliation.
- ADR-0053: projection output is transport machinery.
- ADR-0062: observer catch-up is private read-session machinery.
- ADR-0063: ref resolution owns component-level refs.
- ADR-0038: OP-feed schema identity and app-owned projection keys.
