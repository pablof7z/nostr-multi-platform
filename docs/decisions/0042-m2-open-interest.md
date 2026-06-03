# ADR-0042 — M2 migration: replace `open_author`/`open_thread`/`open_firehose_tag` with generic `open_interest`

- Status: Accepted (mechanism); Proposed (read-path projection — see §5)
- Date: 2026-06-03
- Supersedes the bespoke per-verb feed primitives scheduled for removal in
  `crates/nmp-core/src/kernel/requests/profile.rs` (module doc) and
  `crates/nmp-core/src/kernel/requests/thread.rs` (module doc), per
  `docs/design/subscription-compilation/compiler.md` §3.5 ("M2 full migration").

## 1. Context

Five named C-ABI feed entry points encoded **app-domain decisions inside the
substrate**:

| Deleted C-ABI symbol          | Deleted `ActorCommand` | App decision it smuggled into `nmp-core`                       |
| ----------------------------- | ---------------------- | ------------------------------------------------------------- |
| `nmp_app_open_author`         | `OpenAuthor`           | "an author feed is kinds `{1,6}`" (Chirp social-timeline policy) |
| `nmp_app_open_thread`         | `OpenThread`           | "a thread is kinds `{1,6}` on `#e`"                            |
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

The `{1,6}` literals living in `nmp-core` (and in the `nmp-ffi` shims that
injected them) are a **D0 violation**: the substrate must not name an app
concept like "a social timeline is kind:1 + kind:6". A long-form reader app
would want `{30023}`; a media app `{20,21,22}`. The kind set is app policy.

## 2. Decision — generic `open_interest` / `close_interest`

Two new C-ABI symbols replace the five:

```c
// Register (or attach an owner to) a tailing interest.
// filter_json: standard Nostr REQ filter, e.g. {"kinds":[1,6],"authors":["<hex>"]}
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
re-routes on account switch. Author/thread feeds use scope `0` (re-route on
switch); hashtag feeds use scope `1` (account-agnostic).

## 3. Net symbol delta

−5 (`open_author`, `open_thread`, `open_firehose_tag`, `close_author`,
`close_thread`) +2 (`open_interest`, `close_interest`) = **−3**. The surface
shrinks. Per `docs/wiki/ffi-surface-freeze-gate.md` the freeze gate tracks
net-new symbol *names*; a net-negative delta is within policy. This ADR
satisfies the gate's ADR requirement for the surface change.

## 4. Kept primitives (deliberately NOT migrated)

- `nmp_app_open_uri` — a `nostr:` URI router, genuinely different (resolves an
  entity, not a tailing filter).
- `claim_profile` / `claim_event` / `release_profile` / `release_event` —
  different lifecycle (refcounted one-shot fetch that drives the `claimed_*`
  projections; not a tailing feed). The profile.rs module doc's M2 note that
  `claim_profile` *also* migrates to the registry is **explicitly not done
  here** — claim_* keeps its bespoke lifecycle.

## 5. App-composes pattern for context hydration (read-path)

`open_author` previously co-triggered three things the kernel decided on the
app's behalf: (a) the author's kind:1/6 **notes feed**, (b) a kind:0/kind:10002
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

- **Notes / replies feed** → the host calls `open_interest` with the filter it
  chooses (`{1,6}` for Chirp). The kind set now lives in Swift (D0-correct).
- **Profile card** → `claim_profile(pubkey, "author-page-<pk>", …)` → the
  highest-precedence `claimed_profiles` projection tier (which already exists
  and, per the deleted `projections.rs` precedence `claimed_profiles >
  author_view.profile > mention_profiles`, already overrode the
  `author_view.profile` tier this ADR removes).
- **Thread root hydration** → `claim_event(nostr:nevent…, "thread-root-<id>", …)`
  → `claimed_events`. When the root URI cannot be determined from context the
  call is skipped (a follow-up, not a refactor blocker).
- **Thread navigation affordances** (previous/next counts, root/focused ids)
  and the profile **primary action** (follow/unfollow) → app-composed from the
  feed + follow-state the host already has.

### 5.1 The read path: register through the existing `nmp-feed` seam (NOT a new projection)

The one piece app composition cannot synthesise from existing projections is
**"the store events matching a registered interest"** for a *non-followed*
author or an arbitrary thread/hashtag. The coupling is two-layer, and deeper
than "the read projection vanishes":

1. **Store admission.** `Kernel::should_store_event`
   (`kernel/ingest/timeline.rs`) gates whether an inbound event enters
   `self.events` at all. Its admit clauses are bespoke to the deleted
   machinery: `timeline_authors` (follow set), `author_view.selected_author`,
   and the `author-notes-` / `thread-ids-` / `thread-replies-` /
   `diag-firehose-` sub-id prefixes produced by the deleted request builders.
   A generic `open_interest` registers a planner `sub-<hash>` interest; inbound
   events on that sub for a non-followed author match **none** of the admit
   clauses, so they are never stored.
2. **Exposure.** The home-feed `timeline` / `inserted` / `updated` / `removed`
   delta is computed over `visible_items()`, which iterates the curated
   `self.timeline` id list (follow-feed only), so even a stored
   non-followed-author event never reaches the shell through that cluster, and
   the deleted `author_view`/`thread_view` were its only other exposure path.

**Decision (reuse, not reinvent).** Do NOT add a bespoke `interest_feeds`
kernel projection. A generic, reusable feed-registration seam already exists
and the home feed already runs on it:

- `nmp-feed` (ADR-0033) owns the keyed `FeedRegistry`
  (`register(key, Arc<dyn FeedController>)`), cursors, windowing, and the
  viewport FFI (`load_older`).
- `nmp-feed`'s `RootIndexedFeed<R, A, C>` (ADR-0035) is the protocol-agnostic
  engine; `nmp-nip01::register_op_feed` (ADR-0038) is the NIP-10 instance that
  produces the `nmp.feed.home` snapshot key. The engine ingests via the
  generic `KernelEventObserver` fan-out (`notify_event_observers`, which fires
  for every stored event — `kernel/ingest/timeline.rs:219`).

Author and thread feeds are therefore additional **feed instances registered
under their own keys** (e.g. `nmp.feed.author.<consumer>` /
`nmp.feed.thread.<consumer>`) through the same `register_feed` +
`register_event_observer` composition the home feed uses — or a single generic
filter-feed instance in `nmp-nip01`/`nmp-feed` parameterised by the
`InterestShape` the matching `open_interest` registered. Building a parallel
`interest_feeds` snapshot projection next to this engine would be exactly the
"parallel machinery / substrate theater" the repo forbids; the existing seam is
the architecturally-right home.

**Remaining kernel work (the part this ADR mandates beyond the −3 surface).**
`should_store_event` must be generalised so an event is admitted when it
matches any active registered interest (not only the bespoke follow-set / view
/ sub-id-prefix clauses). This puts shape-matching on the hot ingest path that
the cheap `timeline_authors.contains` short-circuit avoided — a real D8 cost
that argues further for reusing the feed engine's own event gate
(`register_op_feed` already installs an `EventGate` /
`ParentResolver`-driven filter on the observer path) rather than re-deriving
admission in the kernel. The exact gate placement (kernel `should_store_event`
generalisation vs. feed-engine-side gating with a store-retain hook) is the
open sub-decision for the author/thread half; it is recorded in `docs/BACKLOG.md`
as the next-sprint task and does not block the additive PR1 mechanism below.

### 5.2 Why the original task framing under-counted the scope

The task framed this as "−5 +2 = −3 surface, within freeze policy." That is
true for the *subscribe* surface and for the `diagnostic_firehose` verb (which
has zero projection / Swift coupling — a clean mechanical delete). It is **not**
true for the author/thread verbs: their `author_view` / `thread_view`
projections are the sole read path for Chirp's `ProfileView` / `ThreadScreen`
(verified: `ProfileView.swift` derives its note list from `authorView.items`),
and deleting them forces the store-admission + feed-registration work above.
The author/thread read-path migration is the architecture-significant half and
is sequenced as a follow-up sprint (see `docs/BACKLOG.md`).

## 6. Consequences

- D0: the substrate no longer names "social timeline" / "thread" / "hashtag
  feed" or their kind sets. Verified by the absence of residual `[1u32, 6u32]`
  / `{1, 6}` literals in `nmp-core` feed paths after the migration.
- The bespoke `author_view` / `thread_view` / `diagnostic_firehose` kernel
  state, their request builders, per-view refcounts, and
  `close_subscriptions_with_prefixes` are deleted; the generic registry +
  compiler path is the single subscription machine.
- All non-Chirp Rust callers of the five deleted symbols (chirp-desktop,
  chirp-tui, nmp-gallery android/ios/tui, nmp-android-ffi, the ffi-stress
  binaries) migrate to `open_interest`/`close_interest`.
- `ffi-surface.md` and the codegen Swift projection registry are updated;
  generated Swift types are regenerated.
