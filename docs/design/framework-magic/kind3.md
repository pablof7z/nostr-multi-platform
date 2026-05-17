# Framework Magic §C5 — Kind:3 Auto-Tracking

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/design/subscription-compilation/recompilation.md` (trigger model — kind:3 is the symmetric case to `Trigger::Nip65Arrived`); `docs/design/subscription-compilation/intro.md` §2.3 (account scope binding); `docs/plan/scope-adjustments-2026-05-18.md` §"Folded into M2".

## 1. The bullet

### C5. Kind:3 auto-tracking: the active account's follow-list change recompiles every dependent subscription transparently.

**Framework does:**

When a kind:3 event lands for the active account's pubkey *and* the replaceable-supersession rule (C1) decides it is fresher than the stored kind:3, the kernel:

1. Replaces the stored kind:3 in the event store (per C1; mechanism at `crates/nmp-core/src/kernel/ingest.rs:187-207` — currently stored in `self.seed_contacts` map; M2 graduates this into the projection cache).
2. Emits an internal planner trigger — proposed name `Trigger::FollowListChanged { account: AccountId, prev_follows: BTreeSet<Pubkey>, next_follows: BTreeSet<Pubkey> }` — symmetric to the existing `Trigger::Nip65Arrived` (`docs/design/subscription-compilation/recompilation.md` §4.1).
3. The subscription compiler re-runs `interests()` on every `ViewModule` whose `dependencies()` declares `kind 3` *or* whose `interests()` consumes the active account's follow-set as an input to its filter shape (e.g. a "following timeline" view module).
4. The wire-emitter diffs the new plan against the old; only the *delta* (authors added/removed from the union write-relay set) becomes CLOSE / new-REQ frames on the wire. Authors present in both old and new follow-sets see zero wire churn.
5. The view payload's `items` recompute reactively per the standard `on_event_inserted` path (`docs/design/kernel-substrate.md` §3). No view handle is destroyed; the platform shadow's `useFollowingTimeline()` rune/observable continues to emit, just with a new payload.

**App writes:** nothing. The "following timeline" view's spec does not name authors — the view module consumes the active account's follow-set internally. The app's only contact with this surface is opening `FollowingTimelineView { /* no fields */ }` and reading its `Payload.items`.

**Failure mode prevented:** the canonical NDK-era bug: app code listens for kind:3 events, manually closes its open subscriptions, re-derives author lists, re-issues REQs, and either races itself (REQ ordering vs. local-state ordering) or leaks the old REQ. This contract structurally forbids that pattern: the view module never sees the kind:3 directly, and the app never issues a REQ. Specifically discharges aim.md §6 doctrine 6 ("subscriptions auto-group, auto-close, auto-dedup, auto-buffer; the developer never writes grouping/dedup/cleanup code") for the follow-list-change case.

**Test:** `c5_kind3_change_recompiles_follow_dependent_subs` in `crates/nmp-testing/tests/framework_magic_contract.rs`. The test:

1. Opens a `FollowingTimelineView` against an active account whose stored kind:3 follows pubkeys `{A, B, C}` with mailbox cache pre-seeded so A→relay1, B→relay2, C→relay3.
2. Asserts the initial plan opens REQs on `{relay1, relay2, relay3}` and that the platform shadow has emitted exactly one payload.
3. Ingests a fresher kind:3 for the active account with follows `{A, B, D}` (D's mailbox pre-seeded → relay4).
4. Asserts the planner emitted exactly two wire frames: `CLOSE` on the relay3 slice for C, and `REQ` on relay4 for D. Crucially: no churn on relay1 (A is still there) or relay2 (B is still there).
5. Asserts the same `FollowingTimelineView` handle is still open (refcount unchanged); the platform shadow has emitted one additional payload, not torn down and re-created.
6. Asserts a stale kind:3 (older `created_at`) is rejected without firing the trigger — symmetric to C1 supersession; no payload re-emit.

The test runs against the `PlannerHarness` introduced in `docs/design/subscription-compilation/tests.md` §9.3, extended with a `follow_set_for(account)` accessor.

**Milestone owner:** **M2** (the subscription-compilation milestone owns the trigger and the recompile). M2's exit gate (`docs/design/subscription-compilation/tests.md` §9) currently lists four assertions covering the NIP-65 case; the M2 owner adds this fifth assertion as part of the framework-magic delta. Test starts as `#[ignore = "pending M2 trigger"]`; M2 lands the trigger and removes the ignore.

## 2. Why kind:3 is its own bullet (not a sub-case of C1)

Kind:3 is a replaceable event, so C1 already says "the stored kind:3 is the newest." The reason kind:3 deserves a separate bullet is that it is **referentially structural**: it changes which *other authors* the framework needs to subscribe to, not just which version of the kind:3 the app reads. That second-order effect — the change in the *open-subscription set* — is the one apps have historically failed at.

C1 is a storage-layer invariant. C5 is a planner-layer reactive guarantee. The framework needs both.

## 3. NDK reference path

The user's directive in `scope-adjustments-2026-05-18.md` says: *"NDK reference: how NDK auto-follows kind:3 changes and re-routes its open subs. (Captured in M2 research wave; agents fan out.)"*

The mechanism NDK uses is documented in the parallel research file `docs/research/ndk/kind3-auto-tracking.md` (pending agent landing). The contract here does not depend on NDK's specific code path; it depends on the *property* NDK demonstrates: that a kind:3 replacement re-shapes the open-REQ set without the application observing protocol churn.

`TBD-from-research(ndk/kind3-auto-tracking.md)`: insert file:line ref to NDK's listener and the exact race-window it closes (specifically: what happens if a kind:3 arrives mid-EOSE on a follow-derived REQ). The contract is satisfied by *any* mechanism that produces the observable behavior in C5; NDK's path is one existence proof.

## 4. Applesauce reference path

`scope-adjustments-2026-05-18.md` also says: *"Applesauce reference: the 'event store query builder' magic that makes subscriptions auto-update without the app touching them. Highest-priority NDK/Applesauce lesson per user."*

`TBD-from-research(applesauce/event-store-query-builders.md)`: cite the query-builder API shape that lets a consumer phrase `"things kind:1 by people I follow"` once and get a stream that re-evaluates on every kind:3 change. Applesauce's mechanism is a builder that registers itself as a dependent of the kind:3 projection; the contract's `ViewModule.dependencies()` is the NMP analog (`docs/design/kernel-substrate.md` §3 lines 131–132). The research-fold commit cross-validates that the analog covers Applesauce's pattern fully.

## 5. Interaction with NIP-65 (kind:10002)

A new follow (D in the test) needs a mailbox lookup. If D's kind:10002 is not in the mailbox cache, the planner's existing indexer-fallback logic (`docs/design/subscription-compilation/compiler.md` §3 Stage 2) routes D to the indexer set while concurrently fetching D's kind:10002. The fetch eventually triggers `Trigger::Nip65Arrived`, which recompiles again — moving D from the indexer slot to D's declared write relay.

That second recompile is **not part of the C5 test** — it belongs to the M2 NIP-65 audit gate (test #3 in `docs/design/subscription-compilation/tests.md` §9.2). The C5 test asserts kind:3 alone caused exactly the right delta; the NIP-65 chained recompile is a separate observable that the M2 gate already covers.

## 6. What this bullet does not cover

- **The "following timeline" view module itself.** Its spec, payload, recompute logic live in `nmp-nip01` per `docs/design/view-catalog/profile-timeline-thread-reactions.md`. C5 cares only that *whatever view module* declares follow-set dependence gets the recompile.
- **Mute-list changes (kind:10000).** The mute list is structurally analogous, but the user's scope-adjustments doc explicitly names kind:3. Mute-list auto-tracking would be a C5-shaped sibling bullet (potential C14 future addition); not in the v1 contract surface.
- **Other people's follow lists.** A view module that opens kind:3 for `pubkey != active_account` is asking a one-shot question, not declaring a reactive dependency on the social graph. That path uses the normal C1 supersession; no C5 trigger fires.

These exclusions keep the bullet sharp: C5 is exactly *"the active account's follow-list change re-shapes the open-subscription set."* Everything outside that sentence routes through other contract bullets.
