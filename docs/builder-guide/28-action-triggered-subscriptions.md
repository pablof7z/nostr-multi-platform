# 28 — Action-triggered read sessions
> **Status: SHIPS** · Audience: both · Read after [05a — Substrate traits](05a-substrate-traits.md) and [07 — Subscription planner](07-subscription-planner.md).

This chapter closes the gap that sent podcast-player to
`dispatch_capability("nostr_relay", …)`: the kernel had the machinery, but the
app-facing recipe was wrong.

## The gap and why it matters

Production app code should model "user taps something → kernel fetches matching
events → typed output updates" as a typed read session claim/release. The action
records the user's intent to claim or release a session. The session helper owns
the internal acquisition, replay-before-live, output, status, and teardown.

`ActorCommand::EnsureInterest`, `DropInterestOwner`,
`open_observed_projection`, and `ObservedProjectionSink` are implementation
machinery behind that helper. They are not the app-facing production recipe
under ADR-0070.

## The moving parts

Every action-triggered read feature wires the app-visible pieces at **init
time** — before any action is dispatched, before `nmp_app_start`:

```text
typed read-session helper         ←── owns demand + replay + output + teardown
register_action                   ←── dispatches Claim / Release intent
```

The action opens or closes a typed session owner. The session helper may use
logical interests, observed delivery, and typed sidecars internally. The shell
sees typed output and status on pushed frames. Nothing is polling. Nothing is in
the shell.

## The action module: Claim/Release

The action has two variants — claim (open) and release (close) — tagged in one
enum under one namespace. They live in the same module because both must derive
the same `SubIdentity` from the same inputs: a derivation mismatch (drop the
wrong owner) causes a subscription to leak forever.

The helper names below are shape-level placeholders. The invariant is that
product code dispatches Claim/Release intent and the typed session helper owns
the internal acquisition commands.

```rust
// crates/nmp-defaults/src/topic_articles.rs
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum TopicArticlesAction {
    Claim   { topic: String, consumer_id: String },
    Release { topic: String, consumer_id: String },
}

impl ActionModule for TopicArticlesModule {
    const NAMESPACE: &'static str = "nmp.app.topic_articles";
    type Action = TopicArticlesAction;

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            TopicArticlesAction::Claim { ref topic, ref consumer_id } => {
                send(claim_topic_articles_session(topic, consumer_id));
            }
            TopicArticlesAction::Release { ref topic, ref consumer_id } => {
                send(release_topic_articles_session(topic, consumer_id));
            }
        }
        Ok(())
    }
}
```

Shell dispatch JSON:

```json
{"namespace":"nmp.app.topic_articles","action":{"op":"claim","topic":"bitcoin","consumer_id":"discover-view"}}
{"namespace":"nmp.app.topic_articles","action":{"op":"release","topic":"bitcoin","consumer_id":"discover-view"}}
```

## Internal acquisition identity

A typed session helper eventually materializes internal acquisition owners. The
current substrate represents those owners with a `SubIdentity`
(`crates/nmp-core/src/subs/sub_key.rs:127`). This is not app vocabulary, but the
internal triple explains why claim and release must share one derivation:

- **`SubOwnerKey`** — who holds the refcount. One per view instance / call
  site. Multiple owners may attach to the same slot; the registry keeps one REQ
  alive while any owner is attached and GCs the slot when the last drops
  (`registry.rs:103`).

- **`SubKey`** — what slot. Derived from the subscription's logical identity
  (kind + filter discriminant). All owners of the same data share one `SubKey`
  → one REQ on the wire. Multi-lane feeds use one `SubKey` per lane.

- **`SubScope`** — account context. `Global` for content not tied to a
  specific account; `Account(pubkey)` for inbox-style subscriptions scoped to
  one identity.

Derive both with namespaced hashes so keys from different modules never
collide:

```rust
// crates/nmp-defaults/src/topic_articles.rs
pub fn topic_articles_identity(topic: &str, consumer_id: &str) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new((TOPIC_ARTICLES_NAMESPACE, "owner", topic, consumer_id)),
        SubKey::builder(TOPIC_ARTICLES_NAMESPACE).with(topic).finish(),
        SubScope::Global,
    )
}
```

The pattern: `SubOwnerKey` folds in `consumer_id` (owner-unique);
`SubKey` folds in only the content discriminant (owner-shared).
The typed helper owns this derivation so product code does not construct
subscription keys directly.

## Feed declarations use primary kinds

Feed-opening actions and FFI helpers that expose a feed should declare the
content the app wants to render, not every protocol wrapper needed to acquire
it. For a Chirp-style note feed the app says "primary kinds `[1]` from the
current user's reactive follows." For an Olas-style picture feed it says
"primary kinds `[20]` from this relay set / WoT / app-defined admission
function." The protocol adapter compiles that into acquisition kinds:
`nmp_nip18::try_acquisition_kinds_for_primary([1])` yields `{1, 6}`, and
`try_acquisition_kinds_for_primary([20])` yields `{20, 16}`.

Kind `6` and kind `16` are never valid primary feed kinds. They are repost
wrappers derived from the primary target kinds. Supplying them as primary input
is rejected instead of silently expanded, because otherwise the app keeps
describing transport mechanics instead of its render intent.

The perspective is equally declarative and reactive. A feed says "from the
current user's follows", "from the current user's WoT", "from relays A/B/C", or
"from this app predicate." It must not snapshot "these are my follows" in app
state. When a kind:3/contact-list, mute, block, deletion, relay-list, or other
perspective-changing event arrives through normal ingest, the Rust owner of the
perspective recomputes acquisition/admission and the feed window is reset or
updated from that current fact.

Relay-set feeds are no-author feeds. The declaration names primary kinds plus
the relays that define the perspective; subscription compilation emits
kind/relay-scoped acquisition and does not add `authors`, `#p`, `#a`, or `#e`
filters unless the app explicitly declared that different source shape.

Custom filtering, ranking, and sorting are caller-owned policy. A WoT feed, a
relay-set feed, and an app-specific "quality" feed can share the same
acquisition shape while carrying independent admission and ordering closures.
Changing that policy is a perspective change: reset the window and replay from
the current store/pull cursor contract instead of leaving rows admitted under
the old policy on screen.

The feed itself still does not claim secondary data. If a mounted row renders an
avatar, it opens a profile claim for that pubkey. If a button renders a reply
count, that relation-count component opens the dependency it needs. If a repost
row references a missing target, the row/component that wants the target claims
it. The feed primitive's job is admission, canonical row identity, bounded
storage, ordering, and pagination over events that have arrived through the
normal acquisition path.

If the acquisition source itself is dynamic, the action does not snapshot the
current author/tag/id set. It declares the closed source expression and lets an
internal Rust ReducedSource owner materialize child interests. Active-user follows,
NIP-51 list membership, follow packs, and pointer-event target hydration all
have this shape: source interest/state changes, reducer replaces the derived
set, and those children enter the same registry/planner path as the static
materialized interests owned by the session helper.

`load_older` is rendered-progress pagination. It may scan past event-log rows
that are deleted, muted, blocked, superseded, replaced, or rejected by the
current app admission policy. Those rows advance the cursor but do not satisfy
the user action; the controller keeps pulling until the visible window grows or
the current perspective is exhausted.

## Resetting and replacing a feed on a perspective change

A perspective change (account switch, follow-set replacement, WoT-preset change,
a kind:3/mute/block/deletion arriving through ingest) can invalidate every
admission decision a feed has already made. Two narrow, mechanics-only hooks on
`nmp_feed` express this without the app reaching into feed internals:

- **Reset** — `PullFeedController::reset` (reachable by key via
  `FeedRegistry::reset(key)`) rewinds the pull cursor to seq 0 and then clears
  the feed's visible window, returning whether visible state actually changed.
  These are two sequential steps, not one atomic operation (see below). The
  two must stay coordinated: dropping the rows without rewinding the cursor
  would leave older history unreachable on the next `load_older`; rewinding
  without clearing would double-ingest. Wire the visible-state clear with
  `FeedReset` (in production `FlatFeed::reset_for_perspective_change`) via
  `PullFeedController::new_with_perspective`.
- **Replace** — `FeedReplace` evicts a single superseded source by its hex event
  id. The controller calls it automatically for every `LogOp::Replaced` row a
  drain surfaces (the prior version of a replaceable event), so only the current
  version renders; the superseding version is ingested through `apply` like any
  other positive row. Wire it with `PullFeedController::new_with_replacement`, or
  drive it externally by key with `FeedRegistry::replace(key, source_id)`.

Reset/replace are **not** a substitute for open/close (ADR-0042). Releasing an
interest and ensuring a new one is the right move when the *interest itself*
changes (different kinds/authors/relays). Reset is for when the interest is
unchanged but the *admission perspective* shifted, so the same interest must be
re-evaluated from the current fact — it keeps the registered controller and its
interest seam, only the visible window and cursor are rewound. Replace is finer
still: a single replaceable event superseded a prior version, so one source is
swapped without touching the rest of the window.

**`reset` is a sequential two-step, not an atomic operation.** It takes the
pager lock, rewinds the cursor, releases the lock, then calls `FeedReset` to
clear the visible state. The lock is released before the visible clear to
prevent a deadlock if `FeedReset` acquires any internal lock of its own. The
tradeoff: a `load_older` invoked concurrently between the two steps would see
the cursor at seq 0 with the old visible window and double-replay rows. The
host contract is **serialization**, not cross-thread isolation — always invoke
`reset` from the same serialized feed/perspective-update path as `load_older`
(e.g. the same perspective-change callback, not a concurrent thread). A
poisoned pager lock fails closed (no rewind, no clear, returns `false`) rather
than panicking. Absent keys through the registry are likewise a silent `false`.
The hooks name no app primary-kind policy (D0) and hydrate no secondary data
(D11) — the closures the composition root injects own that.

## Internal LogicalInterest materialization

Session helpers materialize planner demand with `ViewDependencies::into_logical_interest`
(`crates/nmp-core/src/substrate/view.rs:67`) — it maps your declared kinds,
authors, tag-refs, and limit onto the planner's `InterestShape`:

```rust
// crates/nmp-defaults/src/topic_articles.rs
pub fn topic_articles_interest(topic: &str) -> LogicalInterest {
    let mut interest = ViewDependencies {
        kinds: vec![KIND_LONG_FORM_ARTICLE],           // kind:30023
        tag_refs: vec![("t".to_string(), topic.to_string())],
        limit: Some(TOPIC_ARTICLES_LIMIT),
        ..Default::default()
    }
    .into_logical_interest(
        topic_articles_interest_id(topic),
        InterestScope::Global,
        InterestLifecycle::Tailing,
    );
    interest.is_indexer_discovery = true;  // route bootstrap through search indexer
    interest
}
```

**`InterestLifecycle::Tailing`** keeps the subscription open; events stream
in live. **`InterestLifecycle::OneShot`** closes the subscription after the
first EOSE — use for one-time lookups. The registry GCs a OneShot slot
automatically; you do not need to dispatch Release.

**`is_indexer_discovery: true`** tells the planner to route the initial
bootstrap through the configured search indexer. Use it for sparse content
kinds (long-form articles, classifieds, wiki pages) where general-purpose
relays hold little. Leave it `false` for inbox-style subscriptions tied to
known pubkeys.

## Stable internal interest IDs

`InterestId` is the registry's slot key at the planner level. Hash the module
namespace plus the content discriminant — never use a random UUID:

```rust
pub fn topic_articles_interest_id(topic: &str) -> InterestId {
    InterestId(stable_hash64((TOPIC_ARTICLES_NAMESPACE, topic)))
}
```

Same inputs → same hash → same slot across restarts. Idempotent re-claims
attach a new owner to the existing slot without opening a second REQ.
If the session opens multiple lanes, each lane gets its own stable `InterestId`
and corresponding internal owner identity, and Release drops every lane.

## Ensure vs set: the internal silent footgun

`ActorCommand::EnsureInterest` calls `InterestRegistry::ensure_sub`
(`registry.rs:68`) — **register-if-absent**. If a slot with the same
`(scope, key)` already exists, the call attaches the new owner but **leaves
the existing filter unchanged**. It returns `false` and triggers no recompile.

This means: if a session helper uses a static content key like `"active"` and
the user changes the query, the second internal ensure silently discards the new
filter. The old query stays on the wire.

**Correct pattern for a query that changes:** use the query itself as the
content discriminant. Different queries → different `SubKey`s → different slots.
On query change, dispatch Release for the old typed session owner and Claim for
the new one:

```swift
// Shell (Swift) — user changes discover query from "bitcoin" to "lightning"
topicArticles.release(topic: "bitcoin", consumerID: "discover-view")
topicArticles.claim(topic: "lightning", consumerID: "discover-view")
```

A `SetInterest` command that calls `set_sub` (`registry.rs:86` — replaces the
filter in place) does not currently exist as an `ActorCommand` variant. If a
session needs in-place filter mutation, raise that as a typed-session gap — do
not work around it by exposing a static subscription key to product code.

## The session executor — populating the read model

The read session helper owns the executor machinery. Today that machinery may
include a declared observed projection and a typed sidecar. The declaration
names the shape before any event is delivered; the kernel registers the sink
muted, opens the declared interest, replays cached/store-backed rows, then
activates future delivery scoped to the same shape.

```text
// Shape declaration (inside the typed read-session helper — not an app recipe):
// 1. shape:  kind:LONG_FORM_ARTICLE  |  bounded replay (depth 128)  |  scoped delivery
// 2. output: Arc<Mutex<DiscoveryState>> — typed state populated by the session executor
// 3. sidecar: register_typed_snapshot_projection("myapp.discover_results", encoder_fn)
// The observed-projection sink and ensure-interest commands are internal
// session-executor machinery — not app-facing API (ADR-0070).
```

The observer fires synchronously on the actor thread. Keep it fast: no I/O, no
blocking, no panics. Production session helpers do not attach to a filterless
all-event fanout; they declare kind, author, id, tag, relay pin, search shape,
source reducer, or bounded dependencies up front.

## The snapshot projection — delivering to the shell

The helper registers a closure that reads from the same `Arc<Mutex<AppState>>`
and emits a typed sidecar. The closure runs when the actor emits a frame after
ingest or another relevant wake:

```rust
let state_snap = state.clone();
app.register_typed_snapshot_projection("myapp.discover_results", move || {
    state_snap
        .lock()
        .ok()
        .map(|s| encode_discovery_projection(&*s))
});
```

The shell reads `typed_projections["myapp.discover_results"]` off the pushed
`SnapshotFrame` in its `apply()` callback. No polling. Edge-triggered by the
actor's `changed_since_emit` flag whenever an event lands. See
[06 — Reactivity contract](06-reactivity-contract.md) and
[ADR-0039](../decisions/0039-push-projection-seam-canonical.md).

## Registration order

```rust
pub fn register(app: &mut impl AppHost) {
    // 1. Install the explicit NIP/protocol stack this app wants.
    install_protocol_stack(app);

    // 2. Read session helper — owns internal interests, replay, output, status,
    // and close. It may use observed delivery internally.
    register_topic_articles_read_session(app);

    // 3. Action module — the shell can now dispatch Claim/Release.
    app.register_action(TopicArticlesModule);
}
```

The session registration does not need to happen before the action registration
for actor safety — both are consulted only when the actor processes a command or
an event, which is after `nmp_app_start`. The constraint is simply that the
shell calls this app-core `register()` once before `nmp_app_start`.

## Multi-owner refcounting in practice

Two views may independently claim the same topic. Per lane, the registry keeps
one slot and one REQ:

```
View A: Claim { topic: "bitcoin", consumer_id: "feed-column" }
  → SubOwnerKey = hash(NAMESPACE, "owner", "bitcoin", "feed-column")
  → SubKey      = hash(NAMESPACE, "bitcoin")
  → registry: slot (Global, key) created, owners = { "feed-column" }

View B: Claim { topic: "bitcoin", consumer_id: "sidebar" }
  → SubOwnerKey = hash(NAMESPACE, "owner", "bitcoin", "sidebar")
  → SubKey      = hash(NAMESPACE, "bitcoin")          ← same slot
  → registry: owners = { "feed-column", "sidebar" }, one REQ

View A closes: Release { topic: "bitcoin", consumer_id: "feed-column" }
  → drop_owner("feed-column")
  → owners = { "sidebar" }  → slot survives, REQ stays open

View B closes: Release { topic: "bitcoin", consumer_id: "sidebar" }
  → drop_owner("sidebar")
  → owners = {}  → slot GC'd, CLOSE sent to relay
```

This is the `ensure_sub` / `drop_owner` contract in
`crates/nmp-core/src/subs/registry.rs:68-120`.

## `is_async_completing` for this pattern

Subscription-opening actions are **synchronously completing** —
`is_async_completing()` defaults to `false` and that default is correct here.
`execute()` records Claim/Release and returns `Ok`. The host spinner clears
immediately. The ongoing session output flow is entirely separate from the
action's completion signal.

`is_async_completing() = true` applies only when the kernel must wait for a
specific terminal event before it can declare success (e.g. a NWC payment
confirmation). For a tailing subscription that streams events indefinitely,
there is no terminal — do not set the flag. See
[05a](05a-substrate-traits.md) §ActionModule for the full stage machinery.

## What NOT to do — the `dispatch_capability` trap

```swift
// BAD: relay logic in the shell (D0 / D4 violation)
let result = nmpAppDispatchCapability(app, #"""
    {"namespace":"nostr_relay","correlation_id":"…","request_json":…}
"""#)
// This backs the kernel into being a passthrough;
// the shell is now deciding relay connections.
```

`dispatch_capability` (`crates/nmp-core/src/capability_socket.rs:33`) is for
facts the *device* provides that the kernel cannot compute: Keychain access,
push tokens, audio session state, file paths. It is not an escape hatch for
Nostr relay operations. The kernel owns all relay connections, always. The
pattern in this chapter is the correct path.

If you find yourself reaching for `dispatch_capability` with a relay-flavoured
namespace, stop and ask: "Can I claim a typed read session from an
`ActionModule` and read the pushed typed output?" The answer is almost
certainly yes.

## Anti-patterns

| Pattern | Problem | Correct form |
|---|---|---|
| Internal ensure with a static key (`"active"`) for a mutable query | New filter silently discarded; old query stays on wire (`ensure_sub` is register-if-absent) | Use the query as the content discriminant; release the old session owner before claiming the new one |
| Claim and Release in separate action modules | Identity derivation can diverge; wrong owner dropped → sub leaks | Keep Claim/Release as tagged variants of one enum under one namespace |
| `dispatch_capability("nostr_relay", …)` | Relay logic enters the shell; kernel becomes a passthrough (D0, D4) | Action module → typed session Claim/Release → pushed typed output |
| Calling `push_interest` / `EnsureInterest` from product code | Bypasses the typed-session owner; no session status, close contract, or route provenance | Route through a typed read-session helper or a runtime controller (see `runtimes.rs`) |
| Observer that blocks or panics | Stalls the actor thread; may corrupt snapshot cadence | Keep observer body O(1), lock briefly, never panic (D6) |
| Polling the projection from the shell instead of reading off the pushed frame | D8 violation (polling); data is already pushed — read it from `apply()` | Claim the typed session and render its pushed typed output |
| Setting `is_async_completing = true` for a subscription-only action | Host spinner waits for a terminal that never arrives | Leave `is_async_completing` at default `false`; the action is done when Claim/Release is recorded |

## Decision tree — which lifecycle?

```
The subscription should close when…
│
├─ …the first EOSE arrives (one-time lookup, e.g. "fetch Alice's relay list")
│   → InterestLifecycle::OneShot
│   → No Release needed; the registry GCs the slot automatically.
│
└─ …the user navigates away / explicitly cancels
    → InterestLifecycle::Tailing
    → Dispatch Release when the view closes; the shell owns the trigger, Rust owns the subscription.
```
## Checklist

- [ ] Claim and Release derive the same internal owner identity from the same inputs.
- [ ] Internal `SubOwnerKey` includes `consumer_id`; `SubKey` does not.
- [ ] Internal `SubKey` matches the filter; `InterestId` is a stable hash, not a UUID.
- [ ] `is_async_completing()` is `false` (default) for subscription-only actions.
- [ ] Any internal observed executor declares its shape, stays cheap, and never panics.
- [ ] Per-open/late-joining projections use kernel replay, not app-side hydration.
- [ ] The typed output reads from session-owned state; tailing sessions have Release.
- [ ] No relay logic, WebSocket code, or `dispatch_capability("nostr_relay", …)` is in the shell.
See also: [05a](05a-substrate-traits.md) · [06](06-reactivity-contract.md) · [07](07-subscription-planner.md) · [16](16-capabilities.md) · [20](20-new-protocol-module.md).
