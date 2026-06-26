# 28 — Action-triggered subscriptions
> **Status: SHIPS** · Audience: both · Read after [05a — Substrate traits](05a-substrate-traits.md) and [07 — Subscription planner](07-subscription-planner.md).

This chapter closes the gap that sent podcast-player to `dispatch_capability("nostr_relay", …)`: the API existed, but the recipe did not.

## The gap and why it matters

`ActionModule::execute` dispatches `ActorCommand`s. The kernel opens Nostr
subscriptions in response to `LogicalInterest`s pushed into the
`InterestRegistry`. Those two facts look disconnected, but **`execute` can
dispatch `ActorCommand::EnsureInterest` directly**. That is the idiomatic path
for "user taps something → kernel fetches matching events → a shell projection updates."

Live references: `crates/nmp-relations/src/visible_relations.rs` (reaction/reply relations) and `crates/nmp-defaults/src/topic_articles.rs` (NIP-23 long-form articles by topic).

## The three moving parts

Every action-triggered subscription wires three things together at **init
time** — before any action is dispatched, before `nmp_app_start`:

```
open_observed_projection          ←── declared shape + replay + scoped delivery
register_snapshot_projection      ←── reads observer state on every tick
register_typed_snapshot_projection←── optional typed sidecar for the same state
register_action                   ←── dispatches EnsureInterest / DropInterestOwner
```

The action opens (or closes) the subscription slots. The observer catches arriving events
and populates an `Arc<Mutex<AppState>>`. The projection reads that state and emits a typed
sidecar the shell sees on the next pushed frame. Nothing is polling. Nothing is in the shell.

## The action module: Claim/Release

The action has two variants — claim (open) and release (close) — tagged in one
enum under one namespace. They live in the same module because both must derive
the same `SubIdentity` from the same inputs: a derivation mismatch (drop the
wrong owner) causes a subscription to leak forever.

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
                send(ActorCommand::EnsureInterest {
                    identity: topic_articles_identity(topic, consumer_id),
                    interest: topic_articles_interest(topic),
                });
                send(ActorCommand::EnsureInterest {
                    identity: topic_article_reposts_identity(topic, consumer_id),
                    interest: topic_article_reposts_interest(topic),
                });
            }
            TopicArticlesAction::Release { ref topic, ref consumer_id } => {
                send(ActorCommand::DropInterestOwner(topic_articles_identity(topic, consumer_id)));
                send(ActorCommand::DropInterestOwner(
                    topic_article_reposts_identity(topic, consumer_id),
                ));
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

## The SubIdentity triple

`ActorCommand::EnsureInterest` and `DropInterestOwner` both take a
`SubIdentity` (`crates/nmp-core/src/subs/sub_key.rs:127`). The triple is:

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
`visible_note_relations_identity` in `nmp-relations/src/visible_relations.rs`
is the production reference.

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
current author/tag/id set. It declares the closed source expression and lets a
Rust ReducedSource owner materialize child interests. Active-user follows,
NIP-51 list membership, follow packs, and pointer-event target hydration all
have this shape: source interest/state changes, reducer replaces the derived
set, and those children enter the same registry/planner path as the static
`EnsureInterest` examples in this chapter.

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

## Building the LogicalInterest

Use `ViewDependencies::into_logical_interest`
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

## Stable interest IDs

`InterestId` is the registry's slot key at the planner level. Hash the module
namespace plus the content discriminant — never use a random UUID:

```rust
pub fn topic_articles_interest_id(topic: &str) -> InterestId {
    InterestId(stable_hash64((TOPIC_ARTICLES_NAMESPACE, topic)))
}
```

Same inputs → same hash → same slot across restarts. Idempotent re-claims
attach a new owner to the existing slot without opening a second REQ.
If the action opens multiple lanes, each lane gets its own stable `InterestId`
and corresponding `SubIdentity`, and Release drops every lane.

## Ensure vs set: the silent footgun

`ActorCommand::EnsureInterest` calls `InterestRegistry::ensure_sub`
(`registry.rs:68`) — **register-if-absent**. If a slot with the same
`(scope, key)` already exists, the call attaches the new owner but **leaves
the existing filter unchanged**. It returns `false` and triggers no recompile.

This means: if you use a static content key like `"active"` and the user
changes the query, the second `EnsureInterest` silently discards the new
filter. The old query stays on the wire.

**Correct pattern for a query that changes:** use the query itself as the
content discriminant. Different queries → different `SubKey`s → different slots.
On query change, dispatch `EnsureInterest` for the new query and `Release`
for the old one:

```swift
// Shell (Swift) — user changes discover query from "bitcoin" to "lightning"
topicArticles.release(topic: "bitcoin", consumerID: "discover-view")
topicArticles.claim(topic: "lightning", consumerID: "discover-view")
```

A `SetInterest` command that calls `set_sub` (`registry.rs:86` — replaces the
filter in place) does not currently exist as an `ActorCommand` variant. If you
need in-place filter mutation, that is a gap to raise — do not work around it
by reusing a static key with `EnsureInterest`.

## The observed projection — populating the read model

Open a declared observed projection for the read model. The declaration names
the shape before any event is delivered; the kernel registers the sink muted,
opens the declared interest, replays cached/store-backed rows, then activates
future delivery scoped to the same shape:

```rust
// In your app's registration function, called before nmp_app_start:
let state = Arc::new(Mutex::new(DiscoveryState::default()));

let state_obs = state.clone();
app.open_observed_projection(ObservedProjection::from_kinds(
    Arc::new(ArticleObserver { state: state_obs }),
    "myapp.discover_results",
    1,
    [KIND_LONG_FORM_ARTICLE],
    128,
));

// Observer impl — cheap, must not panic (D6):
impl ObservedProjectionSink for ArticleObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind == KIND_LONG_FORM_ARTICLE {
            if let Ok(mut s) = self.state.lock() {
                s.ingest(event);
            }
        }
    }
}
```

The observer fires synchronously on the actor thread. Keep it fast:
no I/O, no blocking, no panics. Production read models do not attach to a
filterless all-event fanout; they declare kind, author, id, tag, relay pin,
search shape, source reducer, or bounded dependencies up front.

## The snapshot projection — delivering to the shell

Register a closure that reads from the same `Arc<Mutex<AppState>>` and emits a
typed sidecar. The closure runs on every snapshot tick after any ingest:

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
    let state = Arc::new(Mutex::new(DiscoveryState::default()));

    // 1. Standard NIP stack first if this app wants the social defaults.
    nmp_defaults::register_defaults(app);

    // 2. Declared observed projection before any app action can trigger ingest.
    app.open_observed_projection(ObservedProjection::from_kinds(
        Arc::new(ArticleObserver { state: state.clone() }),
        "myapp.discover_results",
        1,
        [KIND_LONG_FORM_ARTICLE],
        128,
    ));

    // 3. Projection — reads the observer's state.
    app.register_typed_snapshot_projection("myapp.discover_results", {
        let s = state.clone();
        move || s.lock().ok().map(|g| encode_discovery_projection(&*g))
    });

    // 4. Action module — the shell can now dispatch Claim/Release.
    app.register_action(TopicArticlesModule);
}
```

The observer registration does not need to happen before the action
registration — both are consulted only when the actor processes a command or
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
`execute()` enqueues `EnsureInterest` and returns `Ok`. The host spinner clears
immediately. The ongoing event flow is entirely separate from the action's
completion signal.

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
namespace, stop and ask: "Can I open the subscription from an `ActionModule`
and read the results from a projection?" The answer is almost certainly yes.

## Anti-patterns

| Pattern | Problem | Correct form |
|---|---|---|
| `EnsureInterest` with a static key (`"active"`) for a mutable query | New filter silently discarded; old query stays on wire (`ensure_sub` is register-if-absent) | Use the query as the content discriminant; dispatch Release for old query before Claim for new |
| Claim and Release in separate action modules | Identity derivation can diverge; wrong owner dropped → sub leaks | Keep Claim/Release as tagged variants of one enum under one namespace |
| `dispatch_capability("nostr_relay", …)` | Relay logic enters the shell; kernel becomes a passthrough (D0, D4) | Action module → `EnsureInterest` → observer → projection |
| Calling `push_interest` / `EnsureInterest` outside of `ActionModule::execute` or a runtime controller | Bypasses the action stage machinery; no correlation ID; no host visibility | Route through an `ActionModule` or an account-switch runtime controller (see `runtimes.rs`) |
| Observer that blocks or panics | Stalls the actor thread; may corrupt snapshot cadence | Keep observer body O(1), lock briefly, never panic (D6) |
| Polling the projection from the shell instead of reading off the pushed frame | D8 violation (polling); data is already pushed — read it from `apply()` | Register a push projection; read `typed_projections[key]` in the push callback |
| Setting `is_async_completing = true` for a subscription-only action | Host spinner waits for a terminal that never arrives | Leave `is_async_completing` at default `false`; the action is done when `EnsureInterest` is enqueued |

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

- [ ] Claim and Release derive the same `SubIdentity` from the same inputs.
- [ ] `SubOwnerKey` includes `consumer_id`; `SubKey` does not.
- [ ] `SubKey` matches the filter; `InterestId` is a stable hash, not a UUID.
- [ ] `is_async_completing()` is `false` (default) for subscription-only actions.
- [ ] The observed projection declares its shape, stays cheap, and never panics.
- [ ] Per-open/late-joining projections use kernel replay, not app-side hydration.
- [ ] The snapshot projection reads from the observer state; tailing subs have Release.
- [ ] No relay logic, WebSocket code, or `dispatch_capability("nostr_relay", …)` is in the shell.
See also: [05a](05a-substrate-traits.md) · [06](06-reactivity-contract.md) · [07](07-subscription-planner.md) · [16](16-capabilities.md) · [20](20-new-protocol-module.md).
