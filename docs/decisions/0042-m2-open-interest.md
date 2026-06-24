# ADR-0042 — M2 migration: generic interests and dynamic feed read paths

- Status: Accepted (mechanism). Read-path admission/projection finalized by ADR-0057.
- Date: 2026-06-03
- Replaces the bespoke per-verb feed primitives scheduled for removal in
  `crates/nmp-core/src/kernel/requests/profile.rs` and the deleted
  `kernel/requests/thread.rs` stub.

## 1. Context

Five named C-ABI feed entry points encoded **app-domain decisions inside the
substrate**:

| Deleted C-ABI symbol          | Deleted `ActorCommand` | App decision it smuggled into `nmp-core`                       |
| ----------------------------- | ---------------------- | ------------------------------------------------------------- |
| `nmp_app_open_author`         | `OpenAuthor`           | "an author feed is a Chirp note feed plus repost wrappers"     |
| `nmp_app_open_thread`         | `OpenThread`           | "a thread is a Chirp note feed plus repost wrappers on `#e`"    |
| `nmp_app_open_firehose_tag`   | `OpenFirehoseTag`      | "a hashtag feed is kind `{1}` on `#t`"                         |
| `nmp_app_close_author`        | `CloseAuthor`          | (refcounted teardown of the above)                            |
| `nmp_app_close_thread`        | `CloseThread`          | (refcounted teardown of the above)                            |

Each variant drove a bespoke kernel machine (`author_view` / `thread_view` /
`diagnostic_firehose` state, `author_requests` / `firehose_requests` /
`prepare_thread_requests` request builders, per-view refcounting, and the
`close_subscriptions_with_prefixes` string-prefix close path). This duplicates
what the generic `InterestRegistry` + `SubscriptionCompiler` path already does
for every other subscription (`EnsureInterest` / `DropInterestOwner`,
`registry_mut().ensure_sub` / `drop_owner`, `CompileTrigger`).

The kind and wrapper decisions living in `nmp-core` (and in `nmp-ffi` shims)
were a **D0 violation**: the substrate must not name an app concept like "a
social timeline." A feed declaration names primary content kinds and a
reactive source; protocol adapters derive wrapper acquisition and provenance.
A long-form reader app may declare `[30023]`; a media app may declare `[20]`.

## 2. Decision — generic `open_interest` / `close_interest`

Two new C-ABI symbols replace the five:

```c
// Register (or attach an owner to) a tailing interest.
// filter_json: standard Nostr REQ filter, e.g. {"kinds":[30023],"authors":["<hex>"]}
// consumer_id: refcount owner key — deduplicates across call sites
// scope: 0 = ActiveAccount (re-routes on account switch), 1 = Global
void nmp_app_open_interest(NmpApp *app, const char *filter_json,
                           const char *consumer_id, uint32_t scope);

// Detach one owner. Drops the live subscription when the last owner leaves.
void nmp_app_close_interest(NmpApp *app, const char *filter_json,
                            const char *consumer_id, uint32_t scope);
```

Internal routing (`nmp-core`):

- `ActorCommand::OpenInterest { filter_json, consumer_id, scope }` /
  `CloseInterest { … }`.
- Dispatch parses `filter_json` → `InterestShape` via the new
  `InterestShape::from_filter_json` (the inverse of
  `subs::wire::filter_json_for`), builds a `SubIdentity`
  (`owner = hash(consumer_id)`, `key = hash(InterestShape)`,
  `scope` from the param), and calls the **existing**
  `registry_mut().ensure_sub` / `drop_owner` + enqueues
  `CompileTrigger::InvalidateCompile` — byte-for-byte the body the
  `EnsureInterest` / `DropInterestOwner` arms already run.
- Lifecycle is always **`Tailing`** — `open_interest` is a feed subscription,
  never a one-shot.

### 2.1 Deterministic dedup via the `InterestShape` hash

`SubKey = SubKey::new(InterestShape)`. Because every `InterestShape` collection
is a sorted container (`BTreeSet`/`BTreeMap`), two call sites passing the same
filter — regardless of JSON key ordering or array element ordering — produce the
same shape, hence the same `(scope, key)` slot. The registry refcounts owners
and keeps one live subscription. `consumer_id` is the owner: distinct call
sites for the same filter share the live sub and the last `close_interest`
tears it down.

### 2.2 Scope mapping

The `scope` param maps to `crate::planner::InterestScope` on the
`LogicalInterest` (`0 → ActiveAccount`, `1 → Global`). The registry's
`SubScope` (used in the `SubIdentity` key) has no `ActiveAccount` variant, so
`ActiveAccount` folds to `SubScope::Global` for the dedup key — identical to the
existing `InterestRegistry::legacy_scope` convention — while the real
`InterestScope::ActiveAccount` rides on the `LogicalInterest` so the compiler
re-routes on account switch. Contact feeds use the account-routed surface. A
visited author or open thread is keyed to a concrete pubkey/root id and uses
scope `1` (Global); it does not reroute on account switch. Hashtag feeds also
use scope `1`.

## 3. Net symbol delta

−5 (`open_author`, `open_thread`, `open_firehose_tag`, `close_author`,
`close_thread`) +2 (`open_interest`, `close_interest`) = **−3**. The surface
shrinks. Per `docs/wiki/ffi-surface-freeze-gate.md` the freeze gate tracks
net-new symbol *names*; a net-negative delta is within policy. This ADR
satisfies the gate's ADR requirement for the surface change.

## 4. Kept primitives (deliberately NOT migrated)

- `nmp_app_open_uri` — a `nostr:` URI router, genuinely different (resolves an
  entity, not a tailing filter).
- Refcounted reference resolution — a different lifecycle (one-shot fetch that
  drives reference projections; not a tailing feed). ADR-0063 later folded the
  bespoke profile/event claim lifecycle into `resolve_ref` / `release_ref`.

## 5. App-composes pattern for context hydration (read-path)

`open_author` previously co-triggered three things the kernel decided on the
app's behalf: (a) the author's note feed and repost wrappers, (b) a kind:0/kind:10002
**profile + relay-list discovery probe**, and (c) surfaced the result as the
`author_view` snapshot projection carrying both the **profile card** and the
**rendered note list** (`AuthorViewPayload.items`). `open_thread` likewise
carried the reply list via `ThreadViewPayload.items` plus root/focused
navigation counts. These two projections were the **sole** read path for the
Chirp `ProfileView` / `ThreadScreen` (and the gallery equivalents) — they are
gated separately from the home-feed `timeline`/`inserted`/`updated`/`removed`
cluster (which only carries the follow-set), so deleting them leaves those
screens with no event source.

The migration replaces the kernel's bundled decision with **explicit app
composition**:

- **Notes / replies feed** → the app crate declares the primary content kinds
  and source it wants. The protocol adapter compiles that declaration into the
  concrete raw interests, including derived wrapper kinds where needed. Chirp
  declares primary kind `[1]`; the app does not declare wrapper kinds as
  primary feed content.
- **Profile card** → `claim_profile(pubkey, "author-page-<pk>", …)` → the
  highest-precedence `claimed_profiles` projection tier. The deleted author-view
  profile tier no longer participates in profile resolution.
- **Thread root hydration** → decode `nostr:nevent...`, then
  `resolve_ref(namespace=event, key, "thread-root-<id>", ...)` → `refs.event`.
  When the root URI cannot be determined from context the call is skipped (a
  follow-up, not a refactor blocker).
- **Thread navigation affordances** (previous/next counts, root/focused ids)
  and the profile **primary action** (follow/unfollow) → app-composed from the
  feed + follow-state the host already has.

### 5.1 The read path: register through the existing `nmp-feed` seam (NOT a new projection)

The read path is finalized by **ADR-0057**: admission is valid-signature-only,
every validly-signed non-ephemeral event is persisted unconditionally, and
`Kernel::should_store_event` is demoted to a read-time timeline-*view* predicate
(it selects what enters the in-memory curated `self.timeline`, never what is
durably stored). So a non-followed author's events for a generic `open_interest`
are **already in the store**; the only gap is *exposure*, not storage.

The resolution reuses the existing feed seam rather than adding a bespoke
`interest_feeds` kernel projection:

- `nmp-feed` (ADR-0033) owns the keyed `FeedRegistry`, cursors, windowing, and
  the viewport FFI; `RootIndexedFeed<R, A, C>` (ADR-0035) is the
  protocol-agnostic engine and `nmp-nip01::register_op_feed` (ADR-0038) is the
  NIP-10 instance behind `nmp.feed.home`. The engine ingests via the generic
  `KernelEventObserver` fan-out, which fires for every stored event.
- Author and thread feeds are additional **feed instances registered under their
  own keys** (`nmp.feed.author.<pubkey>` / `nmp.feed.thread.<event_id>`) through
  the same registry, observer, and typed-sidecar composition. Chirp's app crate
  owns them: open registers a feed + observer + `NOFS` typed projection and opens
  the matching `open_interest`; close unregisters the dynamic key (emitting one
  `Cleared` row so host caches drop it) and closes the interest.

A parallel `interest_feeds` snapshot projection would be the "substrate theater"
the repo forbids; the existing seam is the architecturally-right home. Per
ADR-0057 there is no store-admission gate to generalise and no shape-matching on
the ingest hot path.

### 5.2 Why the original task framing under-counted the scope

The task framed this as "−5 +2 = −3 surface, within freeze policy." That is
true for the *subscribe* surface and for the `diagnostic_firehose` verb (which
has zero projection / Swift coupling — a clean mechanical delete). It is **not**
true for the author/thread verbs: their `author_view` / `thread_view`
projections were the sole read path for Chirp's `ProfileView` / `ThreadScreen`.
Deleting them forced the feed-registration read-path work above. That
architecture-significant half is now implemented by app-owned dynamic
FlatFeeds and explicit typed-projection teardown.

## 6. Consequences

- D0: the substrate no longer names "social timeline" / "thread" / "hashtag
  feed" or their app feed kind policies. Primary-kind declarations and wrapper
  derivation live above `nmp-core`.
- The bespoke `author_view` / `thread_view` / `diagnostic_firehose` kernel
  state, their request builders, per-view refcounts, and
  `close_subscriptions_with_prefixes` are deleted; the generic registry +
  compiler path is the single subscription machine.
- All non-Chirp Rust callers of the five deleted symbols (chirp-desktop,
  chirp-tui, nmp-gallery android/ios/tui, nmp-android-ffi, the ffi-stress
  binaries) migrate to `open_interest`/`close_interest`.
- `ffi-surface.md` and the codegen Swift projection registry are updated;
  generated Swift types are regenerated.

## Amendment — typed feed sessions (current rule, #1740)

The current app-facing primitive for all feed declarations is the **typed
`FeedParams` feed-session model** defined in `nmp-feed`
(`crates/nmp-feed/src/params.rs`). It provides explicit acquisition /
admission / ranking / window / projection phases plus primary content kinds.
Apps build a `FeedParams` value, validate it with
`nmp_feed::validate_primary_kinds` (fail-closed: wrapper kinds 6/16 and delete
kind 5 are rejected), and open a session via `open_feed` to receive a
`FeedHandle`. **Steps 1–7 (#1740) landed the typed model + validation, the
`NmpApp::open_feed` session registry, the perspective compiler, and the ONE
public C-ABI doorway `nmp_app_open_feed(app, params_json) -> handle_json` /
`nmp_app_close_feed(app, handle_json)` — in the app-composition crate (which can
name the compiler; `nmp-ffi` stays D0-clean).**

**Step 8 (#1740) — the raw interest lane is now INTERNAL/test-only as app feed
surface.** The `nmp_app_open_contact_feed` / `nmp_app_close_contact_feed` C-ABI
active-follows shims are DELETED; the raw wasm feed-verb dispatch strings
(`nmp.kernel.open_interest` / `close_interest`,
`nmp.feed.declare_active_follows` / `clear_active_follows`) are removed from the
router. The generic `nmp_app_open_interest` / `close_interest` C symbols (§2)
remain ONLY as a low-level NON-feed interest seam (avatar / `nostr:` URI
resolution); they are not an app feed-open surface — the ONLY public way to open
a feed is `open_feed`. The `declare_active_follows_feed` /
`clear_active_follows_feed` Rust methods stay as INTERNAL composition glue (the
home-feed wiring + the perspective compiler's `ActiveUserFollows` arm).

The active-user follow feed is **not** expressible through `open_interest` (§2):
its author set is reactive perspective state derived from the active account's
kind:3, re-evaluated on `FollowListChanged`, and re-routed on account switch.
In `FeedParams`, "active-user follows" is expressed as
`FeedScope::ActiveUserFollows` inside `FeedParams.acquisition` — not as a
named verb or a static author list.

Apps/defaults declare via `FeedParams`:

- the primary content kinds they intend to render (`primary_kinds`);
- the reactive source / perspective (`acquisition: FeedScope::ActiveUserFollows`
  or a relay-set expression);
- admission, ranking, window, and projection policy.

The app never supplies concrete follow pubkeys and never declares repost
wrappers as primary feed kinds. Protocol adapters derive wrapper acquisition
(`6` for kind `1`, `16` for non-kind-1 targets) from the declared primary kinds
before the kernel stores the concrete acquisition set for subscription
compilation.

Relay-set feed declarations follow the same `FeedParams` pattern with a
different `acquisition` expression: the app names primary kinds plus the relay
set, and acquisition is scoped/routed to those relays without synthesizing
`authors`, `#p`, `#a`, or `#e` filters. WoT, mute/block, and app-defined
quality rules are caller-owned admission/ranking/sorting policy within
`FeedParams`; they are not new kernel feed kinds and do not change the
primary-kind declaration.

Pagination is admission-aware. `load_older` must make rendered progress, not
merely consume one acquisition page: if a page contains deleted, muted,
blocked, superseded, or otherwise non-admitted rows, the pull controller keeps
advancing through the event log until it either grows the visible window or
reaches exhaustion under the current perspective.

### Historical context — pre-typed declaration verbs (internal/test-only)

Before `FeedParams`, the active-follows feed used a lower-level declaration
pair that is now **internal and test-only**:

```rust
// INTERNAL/TEST-ONLY — not the app-facing surface
app.declare_active_follows_feed([1]);
app.clear_active_follows_feed();
```

The exported C symbols with the old contact-feed names were compatibility shims
that delegated to this declaration path. They must not be used by apps; the
current primitive is `open_feed(FeedParams)` (step 2). Likewise,
`open_interest` / `close_interest` (§2) are substrate-level primitives used
internally by the feed session machinery, not an app-facing feed API.

### New `ActorCommand` variants

- `DeclareActiveFollowsFeed { acquisition_kinds: BTreeSet<u32> }` — installs
  the adapter-derived acquisition kinds for the active-follows declared feed.
- `ClearActiveFollowsFeed` — withdraws the declaration and closes the resulting
  follow-feed interests.

> The `open_author` / `open_thread` names survive correctly as Chirp **app-layer**
> wrappers (`nmp_app_chirp_open_author_feed` / `nmp_app_chirp_open_thread_feed`,
> e.g. `apps/chirp/chirp-tui/src/runtime.rs`). What was deleted is the *kernel*
> `OpenAuthor` / `OpenThread` commands and their bespoke machines (§1); the app
> verbs now compose `open_interest` + a dynamic feed key (§5.1).

### Net symbol delta

`nmp_app_open_timeline` remains retired. Renaming the old feed-open vocabulary to
active-follows declaration vocabulary is a same-responsibility surface
correction; it is not permission to add a second follow-feed API.

### Consequences

- D5 cluster gating is **symmetric**: the home-feed projection cluster
  (`timeline`, `inserted`, `updated`, `removed`) appears only when
  the active-follows feed declaration has concrete acquisition kinds, and
  disappears when the declaration is cleared or receives an empty primary-kind
  set.
- The `timeline_requested` milestone is unaffected: it is flipped by ingest at
  `kernel/ingest/timeline.rs:309-337`, not by the open/close verb.
- All Chirp shells (iOS, Android, TUI, desktop) call the Chirp wrapper or
  app registration path that declares primary kind `[1]`; wrapper kinds are
  derived below that app-facing API.
- `nmp_app_open_timeline` is removed from `nmp-ffi` and `NmpCore.h`. Any
  platform-stable wrapper name that remains for binary/UI compatibility must
  call the active-follows declaration path; it must not preserve a separate
  home-feed code path.
- Completes V-68 Stage 2 (#911).
