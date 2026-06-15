---
title: Profile Resolution
slug: profile-resolution
topic: profile-resolution
summary: "kind:0 (profile) resolution is 100% claim-driven; a claim registers a LogicalInterest through the single recompile chokepoint and inherits outbox routing, implicit kind:10002 discovery, set-cover, and progressive re-route."
tags:
  - reference
volatility: warm
confidence: high
created: 2026-06-14
updated: 2026-06-15
verified: 2026-06-15
compiled-from: code
sources:
  - pr:1436
  - pr:1437
---

# Profile Resolution

Reference for how NMP resolves a kind:0 (profile metadata) event end to end.
Authoritative as of #1436 (kernel/wasm/ffi) + #1437 (iOS), which moved
profile claims onto the subscription registry. Cross-links:
[event-acquisition](event-acquisition.md),
[neg-77-set-reconciliation](neg-77-set-reconciliation.md),
[relay-routing](relay-routing.md).

## Core invariant: 100% claim-driven

There is no proactive or ingest-time kind:0 fetch for third-party pubkeys. A
kind:0 is acquired only when a UI component *claims* the pubkey, and only
becomes UI-visible through that claim — `resolved_profiles` is built
exclusively from `claimed_profiles` (a fetched-but-unclaimed kind:0 sits in
the kernel cache invisible to the UI; `mention_profiles` has been empty since
V-112). Every author-displaying surface therefore self-claims.

A claim is *not* special-cased. It registers a
`LogicalInterest { kinds:[0], authors:[P], limit:None }` through the same
`InterestRegistry` the follow feed, `claim_event`, DMs, zaps, and reactions
use (`crates/nmp-core/src/kernel/requests/profile.rs`,
`register_profile_claim_interest`). Because it is just an authors-filtered
interest, it inherits, for free, everything the one recompile chokepoint
provides: outbox routing, implicit kind:10002 discovery, greedy set-cover
relay minimization, NIP-19 relay hints, and progressive re-route on NIP-65
arrival.

The single chokepoint is
`SubscriptionLifecycle::recompile_and_diff_with_lookup`
(`crates/nmp-core/src/subs/recompile.rs:56`). Every registered interest is
recompiled on each `drain_tick`; higher-order features (follow feed,
profile claim) MUST NOT call any relay-list helper — 10002 acquisition and
per-author routing are intrinsic properties of the infrastructure below them.

## End-to-end flow: a stranger you tap

Tapping an unfollowed author's avatar (`liveness = CacheOk`):

1. **Claim.** `nmp_app_claim_profile` → `ActorCommand::ClaimProfile` →
   `Kernel::claim_profile` (`requests/profile.rs`). The per-pubkey refcount in
   `profile_claims` is bumped (bounded by `MAX_CLAIMS_PER_PUBKEY`, drop-newest
   on overflow). If the profile is not already resident, the claim registers
   the kind:0 interest.

2. **Register + recompile.** `register_profile_claim_interest` installs the
   `LogicalInterest` via `registry_mut().set_sub(identity, interest)` and
   enqueues a `CompileTrigger::ViewOpened`. The next drain runs
   `recompile_and_diff_with_lookup`.

3. **Two things happen in that one recompile:**
   - **(a) Immediate fallback REQ.** The planner's Case A author router
     (`crates/nmp-planner/src/compiler/partition/case_a_authors.rs:116-186`)
     finds no cached NIP-65 mailbox for the stranger, so it routes the kind:0
     REQ to `app_relays` (the `AppRelay` lane, additive and unconditional) and
     — because the claim interest sets `is_indexer_discovery: true` — falls
     back to `bootstrap_indexer_relays` when the author has neither a mailbox
     nor app relays. The card is never blank waiting on discovery.
   - **(b) Batched kind:10002 D3 probe.** The chokepoint's implicit-discovery
     block (`subs/recompile.rs:141-193`) sees the author is in no
     `mailbox_cache` and not in `probed_mailboxes`, and emits a batched
     `kinds:[10002]` REQ to every indexer (`MAILBOX_PROBE_BATCH = 500` authors
     per chunk, `subs/mod.rs`). The author is then marked probed so we do not
     re-REQ every recompile.

4. **NIP-65 arrival → re-route.** When the stranger's kind:10002 lands,
   `ingest_relay_list` fills the `MailboxCache` and calls `on_mailbox_changed`
   (`crates/nmp-core/src/kernel/ingest/mod.rs:611`), which enqueues
   `CompileTrigger::Nip65Arrived { pubkey, created_at }`. The next recompile
   re-routes the kind:0 REQ off the indexer/app-relay fallback onto the
   author's own NIP-65 write relays
   (`case_a_authors.rs:124-134`, the `Nip65` lane). The kind:0 published on
   the author's own relays now resolves.

This is progressive enhancement: the feed paints from the fallback lane
immediately, then refines per-author as each 10002 lands. NMP's transport
pool dials those per-author relays on demand (see *Connection model* below),
so re-routing to a stranger's relays needs zero new capability.

## Why the old design failed

Before #1436, `claim_profile` built kind:0 REQs through a bespoke path —
`profile_claim_request` → `route_outbox_subscription_relays` +
`req_for_relay` — that bypassed the recompile chokepoint entirely. The
consequences:

- The implicit kind:10002 D3 probe never fired for claimed strangers, so
  Lane 1 (the author's own write relays) was inert for anyone but the
  logged-in account. A stranger's kind:0 only ever hit the indexer set.
- The indexer-only routing was gated by an obsolete "kind:0 must not leak
  onto the content relay" contract (`BootstrapSeed::IndexerOnly`). With
  purplepag.es AUTH-walling anonymous queries, the indexer set was
  effectively primal.net only.

Measured impact: of 1054 follows, indexer-only resolution resolved 108 kind:0
profiles (~10%, primal.net only); outbox routing (indexer ∪ own-relays)
resolved 528 (~50%) — a ~5× improvement, capped by the ~58% of follows who
actually publish a NIP-65 list.

`drain_pending_reverify` (F-TTL re-verification of stale replaceables) had the
identical bespoke-bypass defect and was migrated onto the registry in the same
PR. A codebase audit found the anti-pattern isolated to this profile-claim
family; `claim_event`, DMs (NIP-17), zaps (NIP-57), reactions, contacts, and
the follow feed already used the chokepoint. `claim_event` is the reference
implementation the migration mirrored.

The bespoke `profile_claim_request`, `pending_profile_claim_requests`, the
`ProfileRequestState`/`profile_requests` machine, and
`refresh_profile_after_mailbox` were all deleted; the `Nip65Arrived` recompile
replaces the old requested→pending re-queue.

## Liveness hint

The claim seam carries a freshness hint that selects the registered interest's
lifecycle (`ProfileLiveness`, `requests/profile.rs`):

| Liveness | Lifecycle | Behaviour | Caller |
|----------|-----------|-----------|--------|
| `CacheOk` (0) | `OneShot` | serve from cache; one kind:0 fetch on miss, closes on EOSE; no live sub | feed avatars |
| `Live` (1) | `Tailing` | kind:0 sub stays open while claimed so profile edits (kind:0 replacements) arrive reactively | open profile screen |

Mixed claims on one pubkey resolve to **Tailing wins**: a `Live` claim
upgrades an existing `CacheOk` slot in place (the kernel tracks Live pubkeys
in `live_profile_claims`, and `set_sub` re-installs the interest while
preserving the owner set), and the slot stays `Tailing` until the last owner
releases (downgrade only on full teardown). Both liveness levels share ONE
`(SubScope::Global, profile-claim:<pubkey>)` slot, so they dedup to a single
wire REQ.

Warm-reclaim invariant: a `CacheOk` claim for an already-resident profile does
NOT register a network-fetching interest — the resident store serves the card
and the F-TTL gate (`claim_replaceable`, keyed on `ReplaceableKey`, independent
of the claim path) owns re-verification. A `Live` claim still registers, because
it wants future kind:0 edits.

The FFI is 5-arg (no new symbol — `liveness` was added to the existing
`nmp_app_claim_profile`, `crates/nmp-ffi/src/timeline.rs`):

```c
void nmp_app_claim_profile(void *app, const char *pubkey,
                           const char *consumer_id, int force, int liveness);
```

`ProfileLiveness::from_ffi` maps `0 → CacheOk`, anything else `→ Live`. On
iOS (#1437): `NostrAvatar` passes `.cacheOk`, `ProfileView` passes `.live`;
`NostrProfileName`, `NoteContentView` mention claims, and `HomeFeedView`'s
`ReplyAttributionLine` self-claim with `.cacheOk`. The
`NostrProfileHost` protocol defaults to `.cacheOk` so call sites that do not
care stay clean.

### Self-claiming surfaces (the #1437 coverage fix)

The other half of the ~50% miss rate was UI claim-coverage gaps: pre-#1437
only `NostrAvatar` and `ProfileView` claimed. `NostrProfileName`,
note-content mentions, and reply/repost attribution lines passively read the
profile map and never claimed, so their kind:0 was never surfaced even when
cached. #1437 added refcounted self-claims (mirroring `NostrAvatar`'s
`.task(id:)` / `.onDisappear` pattern) to those surfaces. The rule: **every
author-displaying component self-claims.**

## Retry-on-miss: probed_mailboxes re-arm gating

`probed_mailboxes` is insert-only — an author probed once (even if the 10002
EOSE came back empty, or the indexer was down at probe time) is not re-probed
on every recompile. The retry mechanism re-arms (clears) the set only on a
**genuine indexer reconnect** or a **newly-added indexer**:

- **Genuine reconnect.** `relay_connected_url`
  (`requests/relay_lifecycle.rs:38`) captures the prior per-URL transport
  state BEFORE marking it connected, via `indexer_socket_was_down`
  (`relay_transport.rs`), which is true only when the URL was `backing_off`
  (failed) or `closed` (torn down). On a genuine reconnect it calls
  `clear_probed_mailboxes` and enqueues an `IndexerSetChanged` recompile, so
  still-uncached authors are re-probed. Resolved authors short-circuit on the
  mailbox-cache hit, so the re-probe is bounded by the genuinely-unresolved
  set, not a storm.
- **New indexer added.** Handled separately by `IndexerSetChanged` in
  `set_configured_relays` (clears the probe set `if changed`).

Why the gating matters: a naive clear-on-*every*-connect fires on the first
(normal) startup connect and on redundant duplicate connects of an
already-live socket. Each clear forces an `IndexerSetChanged` recompile
mid-load, churning the feed subscription. Under the single-threaded wasm
runtime this starves the UI so notes never paint (the #1436 web-feed
regression). The `was_down` guard restricts the re-arm to reconnect-after-down
only.

## The web/wasm snapshot rule

Claim/release dispatch on the wasm runtime MUST NOT push a kernel snapshot
(`crates/nmp-wasm/src/runtime.rs`, the `dispatch` claim arm; routing in
`runtime/dispatch.rs`). The arm fans any outbound to the relay drivers and
returns `WorkerEvent::ActionAccepted` only — no `UpdateBytes`.

The reason is an architectural hazard, not a micro-optimization: on the web,
SolidJS `<For>` remounts `NostrAvatar`/`NostrProfileName` on each snapshot,
and those components re-dispatch claim/release in `onMount`/`onCleanup`.
Pushing a snapshot on claim hands the reactive host a fresh frame on every
claim → the `<For>` rebuilds its rows → remounts the components →
release + re-claim → an unbounded claim → snapshot → re-render → claim loop
(observed: 170k+ frames, 16k+ alternating claim/release) that, on the
single-threaded wasm worker, floods the main thread and OOM-crashes the
renderer so the feed never paints.

Claim/release are refcount bookkeeping carrying no user-visible data of their
own; the resolved kind:0 arrives later via the relay-pool ingest sink, which
pushes its own snapshot. **Rule for any future dispatch arm: an arm that only
mutates refcount/registry bookkeeping must ACK without pushing a snapshot;
only data-bearing ingest drives the next render.**

## Relay-set minimization (set-cover)

The naive plan connects to every NIP-65 write relay declared by every author
(hundreds in real data). On every recompile, before wire emission, the planner
applies greedy weighted max-coverage with a per-author redundancy cap
(`apply_selection_with_lookup`, `subs/recompile.rs:101`; algorithm in
`crates/nmp-planner/src/selection.rs`). It reduces the relay set to roughly
`select_max_connections` with each author covered by at most
`select_max_per_user` relays. The optional W4 score-lookup filters to warm
outbox relays for authors that have a warm option (see
[neg-77-set-reconciliation](neg-77-set-reconciliation.md)). No set-cover code
is profile-specific; kind:0 claims inherit it because they are ordinary
authors-filtered interests.

Author-union coalescing further collapses avatar claims: the claim interest
uses `limit: None` (kind:0 is replaceable, so the missing explicit limit is
harmless — one event per author maximum), which lets same-shape author sets
merge into ONE batched `authors:[…]` REQ per relay (the merge lattice refuses
to coalesce any shape carrying a `limit`). The migration needed zero
`nmp-planner` changes for this reason.

## Connection model

Per-author outbox dialing is real and bounded. The router's Lane 1
(`crates/nmp-router/src/router.rs`) resolves each author to their NIP-65
write set on publish / read set on subscribe (`MailboxCache`); Lane 6
(Indexer) is always-on for discovery kinds (0/3/10000–19999) and defeats the
kind:10002 self-sealing loop; Lane 7 (AppRelay) is the final fallback.

The transport pool is URL-keyed: a kernel frame targeting a not-yet-open URL
spawns a worker on demand (`crates/nmp-core/src/actor/relay_mgmt.rs`,
`ensure_relay_worker_with_kind`). On-demand sockets are tagged
`RelayConnectionKind::Temporary` (vs `Persistent` for user/app/session-owned
sockets) and are torn down after the kernel reports no active demand for their
URL, past a `TEMPORARY_RELAY_IDLE_GRACE` of 60s
(`crates/nmp-core/src/actor/relay_idle.rs`, `sweep_temporary_idle_relays`).
So connecting to an arbitrary stranger's relays for a one-off kind:0 fetch is
free and self-cleaning.
