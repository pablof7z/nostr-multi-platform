# Framework Magic §C5 — Active-Follows ReducedSource Tracking

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/design/subscription-compilation/intro.md` and
> `docs/builder-guide/21-framework-magic.md` (current ReducedSource model).

## 1. The bullet

### C5. Active-follows ReducedSource tracking: the active account source change recompiles dependent interests transparently.

**Framework does:**

When a kind:3 event lands for the active account's pubkey and the replaceable
supersession rule (C1) accepts it as current, the framework:

1. Stores/replays the source event through the normal accepted-event path.
2. Lets the NIP-02/defaults reducer update the session's `ActiveUserFollows`
   ReducedSource state from the event's `p` tags.
3. Replaces that source owner's materialized child interests through the generic
   dependent-interest owner (`ReplaceDependentInterestSet`), so removed authors
   are withdrawn and added authors are acquired immediately.
4. Lets the existing registry/planner/router diff produce the actual `CLOSE` /
   `REQ` delta on the wire. Authors present in both old and new source outputs
   see zero wire churn.
5. Recomputes the feed payload through the feed-session projection path. The
   feed handle remains valid; the platform receives an updated payload, not a
   teardown/reopen cycle.

**App writes:** open one typed feed session, for example `FeedParams {
acquisition: FeedScope::ActiveUserFollows, primary_kinds: [1], ... }`. The app
does not watch kind:3, does not pass concrete follow pubkeys, and does not issue
identity-change repair calls.

**Failure mode prevented:** the canonical NDK-era bug: app code listens for
kind:3 events, manually closes open subscriptions, re-derives author lists,
re-issues REQs, and either races itself or leaks the old REQ. This contract
structurally forbids that pattern: the app never learns or owns the concrete
expansion. It discharges **D3**, **D4**, and the auto-group/auto-close property
of **C8** for the source-change case.

**Test:** `c5_kind3_change_recompiles_follow_dependent_subs` in `crates/nmp-testing/tests/framework_magic_contract.rs`. The test:

1. Opens a `FollowingTimelineView` against an active account whose stored kind:3 follows pubkeys `{A, B, C}` with mailbox cache pre-seeded so A→relay1, B→relay2, C→relay3.
2. Asserts the initial plan opens REQs on `{relay1, relay2, relay3}` and that the platform shadow has emitted exactly one payload.
3. Ingests a fresher kind:3 for the active account with follows `{A, B, D}` (D's mailbox pre-seeded → relay4).
4. Asserts the planner emitted exactly two wire frames: `CLOSE` on the relay3 slice for C, and `REQ` on relay4 for D. Crucially: no churn on relay1 (A is still there) or relay2 (B is still there).
5. Asserts the same `FollowingTimelineView` handle is still open (refcount unchanged); the platform shadow has emitted one additional payload, not torn down and re-created.
6. Asserts a stale kind:3 (older `created_at`) is rejected without firing the trigger — symmetric to C1 supersession; no payload re-emit.

The current feed-session behavior proof lives in
`crates/nmp-defaults/tests/feed_session_reduced_source_behavior_test.rs`; the
framework-magic contract test keeps the public guarantee pinned.

**Milestone owner:** M2 created the generic dependent-interest owner and M5
(#2092/#2108) migrated active-user follows onto it. This chapter no longer
specifies the removed runtime view-module trigger mechanism.

## 2. Why kind:3 is its own bullet (not a sub-case of C1)

Kind:3 is a replaceable event, so C1 already says "the stored kind:3 is the newest." The reason kind:3 deserves a separate bullet is that it is **referentially structural**: it changes which *other authors* the framework needs to subscribe to, not just which version of the kind:3 the app reads. That second-order effect — the change in the *open-subscription set* — is the one apps have historically failed at.

C1 is a storage-layer invariant. C5 is a planner-layer reactive guarantee. The framework needs both.

## 3. NDK reference path

The user's directive for this contract named the NDK reference: *"how NDK auto-follows kind:3 changes and re-routes its open subs."*

Research conclusion (`docs/research/ndk/kind3-auto-tracking.md`): **NDK has no unified kind:3 → open-subscription rewire in core.** The session package (`@nostr-dev-kit/sessions`) opens a long-lived REQ on the active user's pubkey at `sessions/src/store.ts:184-194` and updates `session.followSet` on each newer kind:3 via `handleContactListEvent` at `store.ts:492-512`. However, this state write does not mutate any open subscription's `authors` filter — filters are immutable after `ndk.subscribe()` returns. Svelte gets implicit rewire via runes (`svelte/src/lib/builders/subscription.svelte.ts:164-177`); React requires explicit dependency declaration (`react/src/subscribe/hooks/subscribe.ts:110`). NMP fills that gap inside Rust: the source owner observes the current follow-set, reduces it to materialized interests, and replaces the child set without a host teardown/rebuild.

The race window NDK Svelte has: `restart()` at `subscription.svelte.ts:117` does `stop()` then a fresh `ndk.subscribe()` — events arriving between the CLOSE and the new REQ's EOSE are missed or re-fetched. NMP's wire-emitter delta-patch (CLOSE only the removed-author slices, open new slices for added authors) avoids this window entirely.

## 4. Applesauce reference path

Research conclusion (`docs/research/applesauce/event-store-query-builders.md`): the "magic" is emergent from four composing mechanisms — typed replaceable-event memory, per-model `share()` with hash-keyed dedup, refcount-based `claim()` GC anchoring, and `switchMap`-into-per-element-subscriptions for the outbox model.

The specific API that produces `"things kind:1 by people I follow"` reactivity is `EventModels.model(OutboxModel, user, opts)` at `event-models.ts:50-86`. When kind:3 arrives for `user`, `ReplaceableModel({kind:3, pubkey:user})` re-emits (`models/base.ts:136-143`), `ContactsModel(user)` maps to a `ProfilePointer[]`, and `OutboxModel` switchMaps each pointer into its `store.replaceable({kind:10002, pubkey})` — so adding/removing a contact spawns/terminates exactly the inner sub for that contact's mailbox with zero app involvement (`models/outbox.ts:14-24`, `observable/relay-selection.ts:19-49`).

NMP's analog is a ReducedSource resolver plus dependent-interest replacement:
the contract's observable — source change causes exactly the right wire delta
without app code — is the same; the mechanism is an actor-owned source owner,
not host-side RxJS/runes or a public view-module trigger.

## 5. Interaction with NIP-65 (kind:10002)

A new follow (D in the test) needs a mailbox lookup. If D's kind:10002 is not in the mailbox cache, the planner's existing indexer-fallback logic (`docs/design/subscription-compilation/compiler.md` §3 Stage 2) routes D to the indexer set while concurrently fetching D's kind:10002. The fetch eventually triggers `Trigger::Nip65Arrived`, which recompiles again — moving D from the indexer slot to D's declared write relay.

That second recompile is **not part of the C5 test** — it belongs to the M2 NIP-65 audit gate (test #3 in `docs/design/subscription-compilation/tests.md` §9.2). The C5 test asserts kind:3 alone caused exactly the right delta; the NIP-65 chained recompile is a separate observable that the M2 gate already covers.

## 6. What this bullet does not cover

- **The "following timeline" view module itself.** Its spec, payload, recompute logic live in `nmp-nip01` per `docs/design/view-catalog/profile-timeline-thread-reactions.md`. C5 cares only that *whatever view module* declares follow-set dependence gets the recompile.
- **Mute-list changes (kind:10000).** The mute list is structurally analogous, but the original user directive explicitly named kind:3. Mute-list auto-tracking would be a C5-shaped sibling bullet (potential C14 future addition); not in the v1 contract surface.
- **Other people's follow lists.** A view module that opens kind:3 for `pubkey != active_account` is asking a one-shot question, not declaring a reactive dependency on the social graph. That path uses the normal C1 supersession; no C5 trigger fires.

These exclusions keep the bullet sharp: C5 is exactly *"the active account's follow-list change re-shapes the open-subscription set."* Everything outside that sentence routes through other contract bullets.
