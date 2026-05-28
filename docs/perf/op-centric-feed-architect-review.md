# OP-centric home feed architecture review

## 1. Top-line verdict

v3 is substantially better than v1/v2, but it is still not
implementation-ready.

The revision correctly fixes the original `ProfileDisplay` cycle in the
`AttributionPayload` trait, moves root hydration toward the existing
`claim_event` primitive, expands the Swift migration list, fixes ADR numbering,
and gives the proposal a much clearer test surface. Those are real
improvements.

The remaining problems are not cosmetic. v3 introduces a new dependency-design
blocker around `FollowSetLookup`, still over-promises the root-claim routing and
release signal, misses several `TimelineBlock::Standalone` Rust consumers, and
leans on startup/account-change seams that do not exist in the current code.
There is also a repost hydration claim that is contradicted by the cited
implementation.

## 2. Strong points

- **B1 is fixed in the trait shape.** The §3-C `AttributionPayload` definition
  uses `type Profile` and does not name `ProfileDisplay`, `AuthorDisplay`, or
  an `nmp-nip01` type. The concrete NIP-10 instance names `ProfileDisplay` only
  in `nmp-nip01`, which removes the original reverse-edge cycle.
- **The root-hydration direction is much closer.** Replacing the bespoke
  `nmp.nip01.thread_root.claim` action with the existing
  `nmp_app_claim_event` / `Kernel::claim_event` path is the right instinct.
  The FFI symbol exists at `crates/nmp-ffi/src/timeline.rs:133-149`, and
  `Kernel::claim_event` does parse event/address `nostr:` URIs, refcount
  `event_claims`, call `OneshotApi::request`, and register claim expansion.
- **`ClaimRequest` carrying `ThreadPointer` is the right future shape.**
  NIP-22 comments use uppercase root-scope tags and can scope to events,
  addresses, or external `I` tags, so event-id-only claims would have been too
  narrow. Source checked: https://github.com/nostr-protocol/nips/blob/master/22.md.
- **B3 is much better scoped.** v3 now names the main Swift consumers:
  `TimelineBlock.swift`, `ModularTimelineBridge.swift`, `HomeFeedView.swift`,
  `ModularBlockView.swift`, generated Swift, and fixtures. That is the right
  all-at-once migration posture.
- **M1 is concrete enough.** The visible-window snapshot test with 5,000 roots,
  `limit = 80`, exact card count, bounded JSON size, and capped internal maps is
  specific enough for an implementer to write.
- **E1/E2/E3 are fixed.** The proposal consistently says seven rungs, ADR-0035
  and ADR-0036 are currently free, and doctrine references point at
  `docs/product-spec/doctrine.md`.

## 3. Concerns / remaining issues / new issues introduced by v3

### Blocker, v3-introduced: `FollowSetLookup` in `nmp-feed` creates an impossible planner/core boundary

v3 moves `FollowSetLookup` to `nmp-feed`, but the same section says the planner
must consume `Arc<dyn FollowSetLookup>` while `nmp-planner` "does not currently
depend on `nmp-feed` and won't need to." Rust cannot type-check a trait object
whose trait crate is not imported.

Adding `nmp-planner -> nmp-feed` is not viable as written because the current
graph is `nmp-feed -> nmp-core -> nmp-planner`; that would form
`nmp-planner -> nmp-feed -> nmp-core -> nmp-planner`. Carrying
`Arc<dyn FollowSetLookup>` through an `nmp-core` compile-context bundle would
also force `nmp-core` to name the trait, contradicting the H1 claim that
`nmp-core` gains no follow-set noun.

Fix direction: either put the trait in `nmp-planner` or a lower substrate crate,
or do not make the planner consume the trait. A cleaner option is to have
`nmp-nip02` / `nmp-app-template` expand the active follow set into concrete
`LogicalInterest`s before the planner sees them.

### Blocker: B2 still over-promises routing hints and no-match release

The happy path through `claim_event` exists, but §3-B step 7 is not what the
current code does.

`OneshotApi::request` constructs a `LogicalInterest` with `hints: Vec::new()`.
`claim_event` extracts relay TLVs from the URI into `uri_relay_hints`, but those
are passed to `register_claim_expansion`, not into the initial planner
interest. Therefore `route_hints` cannot include Alice's provenance or nevent
relay hint on the first OneShot. The initial relay set is
`bootstrap_content_relays`; URI relay hints are only candidates for later
claim-expansion Phase 2 after the Phase 1 budget elapses.

Step 10 is also hand-waved. `complete_unknown_oneshot` releases the registry
owner on EOSE, but it does not remove `event_claims`, clear
`event_claim_requested`, or surface an `event_claim_released` projection. I
found no `event_claim_released` projection. The only public cleanup path is
`nmp_app_release_event` -> `Kernel::release_event`, and that is host/engine
initiated. Existing code gives the engine no no-match signal it can observe.

There is one more mismatch: §3-B says the `claim_expansion_match_author` check
lets Bob's OP through ingest. For the initial discovery oneshot, storage is
allowed by `is_discovery_oneshot(sub_id)`, not by claim-expansion author
matching. With no author TLV, the claim-expansion author match is usually
`None`.

### High: B3 still misses non-Swift consumers

The v3 migration list omits several Rust consumers that will break when
`TimelineBlock::Standalone(EventId)` becomes
`Standalone { id, root }`:

- `crates/nmp-feed/src/types.rs:87-93` implements `FeedBlock` by matching
  `Self::Standalone(id)`.
- `crates/nmp-nip01/src/timeline_projection/tests.rs` has multiple
  `TimelineBlock::Standalone(...)` expectations.
- `apps/chirp/nmp-app-chirp/tests/end_to_end.rs:130` matches
  `TimelineBlock::Standalone(_)`.
- `apps/chirp/chirp-tui/src/timeline/tests.rs` has many JSON fixtures using
  `{"Standalone": "note"}`.

The Notes app does not appear to consume `TimelineBlock`, but the rung-2 file
list is still not consumer-complete.

### High: H2 relies on startup and account-change seams that are not real yet

The proposal says cold start can immediately use LMDB-restored
`timeline_authors`. Current `Kernel::new` initializes both `seed_contacts` and
`timeline_authors` empty. I found no restore path that hydrates either from
LMDB on kernel construction. Startup registers a tailing self-kind subscription
for kind:3, and `ingest_contacts` later rebuilds `timeline_authors` after the
network event lands.

The account-switch story is also speculative. `Kernel::active_account_pubkey()`
does not exist today, and neither does a `KernelAccountChanged` projection
event. There is an internal `CompileTrigger::ActiveAccountChanged` enum case
and an `active_account_handle()`, but the proposal does not include a concrete
observer path that lets the `nmp-nip02` adapter call
`RootIndexedFeed::reset_for_identity_change()` without polling.

### High: H3 repost hydration is not supported by the cited code

L-1 is mostly correct: `TimelineEventCard::from_event` handles embedded reposts
through `RenderPayload::from_event`.

L-5 is not correct. For an e-tag-only repost,
`RenderPayload::from_event` returns an empty placeholder because there is no
embedded event. `try_from_repost_event` does not read the kernel store, and
`TimelineEventCard::from_event` cannot later "pick up the inner note from the
kernel store" on its own. If the target event later arrives, the new engine
needs an explicit replacement/rebuild rule.

L-2 also needs more machinery than the proposal admits. Attributing a reply to a
kind:6 wrapper's target requires the engine to look up the parent event and run
`resolver.supersedes(parent)`. The proposed `Inner` state has no explicit
event-store lookup callback. If this stays in scope, add that callback and tests;
otherwise leave replies-to-reposts keyed to the actual NIP-10 parent event.

### Medium: `ClaimRequest::Release` and kernel cleanup are underspecified

The proposal says pending-map eviction emits `ClaimRequest::Release`, and the
host adapter calls `nmp_app_release_event`. That clears `event_claims` and
`event_claim_requested` only when the last consumer releases. It does not call
`release_claim_expansion`, even though `claim_expansion.rs` documents such a
cleanup hook. If scrolling a row out should cancel retargeting work, the kernel
release path needs to call it or the proposal must explicitly accept
budget-driven cleanup.

### Medium: serialization bounds are implicit

The trait shape does not need `A::Profile: Serialize` unless the profile cache
crosses FFI, but `RootFeedSnapshot<C, A>` does need explicit
`C: Serialize` and `A: Serialize` bounds. The proposal shows
`Nip10ReplyAttribution` deriving `Serialize`, but it does not state the generic
snapshot bounds.

### Medium: `LogicalInterest::SocialTimeline` is internally inconsistent

§7-Q2 says to avoid converting `LogicalInterest` into an enum and instead add a
discriminator field. §5 still says "Add `LogicalInterest::SocialTimeline`" as
if it were an enum variant. Pick one concrete shape before implementation.

## 4. Specific question-by-question

1. **Should `RootIndexedFeed<R, A>` live in `nmp-feed`?** Yes, for the engine.
   But `FollowSetLookup` cannot live there if the planner must consume it
   without adding a cycle.
2. **Is `AttributionPayload` generic enough?** Mostly yes. B1 is fixed. Add
   explicit snapshot serialization bounds for `C` and `A`.
3. **Is the `ClaimRequest` callback pattern correct?** Partly. Carrying
   `ThreadPointer` is right. Calling `claim_event` is right. The initial hint
   route and no-match engine signal are still not real.
4. **Is `FollowSetLookup` over `timeline_authors` adequate?** The data source
   is plausible, but the trait placement is not. Also the proposal overstates
   cold-start restoration of that data.
5. **Will Bob's OP fetch work?** It may work through bootstrap content relays.
   It will not initially ask Alice's provenance relay via `route_hints` unless
   `claim_event`/`OneshotApi` is changed to put URI relays into
   `LogicalInterest.hints`.
6. **Is the sequencing safe?** Safer than v1, but not yet. Rung 2 misses Rust
   consumers, rung 1/rung 4 need a real account-change seam, and the
   SocialTimeline shape contradicts itself.
7. **Does the design miss NIP-10, NIP-18, or NIP-22 cases?** NIP-22 is much
   better after `ThreadPointer`. The naddr trace is inaccurate: current
   `claim_event` does not populate `InterestShape::addresses`; it uses
   `kinds + authors + #d`, which still routes through author outbox when
   possible. NIP-18 repost edge cases need the fixes above.
8. **Open decision defaults.** Q1/Q3/Q4/Q5/Q6 are reasonable defaults. Q7 is
   acceptable only if the proposal stops claiming LMDB-restored
   `timeline_authors` is available at cold start. Q2 must be made concrete.
9. **Missed concerns.** Remaining misses: follow-set trait dependency
   placement, claim no-match signaling, complete TimelineBlock consumer list,
   account-change observer, release/cancel cleanup, repost target hydration,
   and serde bounds.
10. **Doctrine alignment.** Not yet. D0/D3 depend on resolving the
    `FollowSetLookup` placement and claim-hint path. D5 depends on separating
    visible-window snapshot bounds from claim lifecycle cleanup. D8 is fine if
    the identity-change observer is push-based, not polling.

New v3 checks:

1. **`Kernel::active_account_pubkey()` public API.** It does not exist today.
   Adding a substrate-named accessor is probably doctrine-clean, but it must be
   explicitly listed in rung 1 if used.
2. **`KernelAccountChanged` projection event.** Invented. The proposal must
   either implement it or use an existing push seam.
3. **`nmp-nip02 -> nmp-feed` dep edge.** That direct edge alone is probably
   okay. The cycle appears when `nmp-planner` or `nmp-core` also has to name
   `nmp-feed::FollowSetLookup`.
4. **`Profile` associated type and serde.** B1 is clean. Snapshot serde bounds
   still need to be stated.
5. **`ClaimRequest::Release` path.** Engine-to-host release exists only as a
   proposed callback; kernel no-match-to-engine release does not exist.

## 5. Recommendations with concrete edits

1. Move `FollowSetLookup` to `nmp-planner` or a lower substrate crate, or delete
   planner-level SocialTimeline expansion and have `nmp-nip02`/`nmp-app-template`
   register concrete per-follow interests.
2. Fix §3-B: either extend `claim_event` / `OneshotApi::request` so URI relay
   hints become initial `LogicalInterest.hints`, or say clearly that the first
   request hits only `bootstrap_content_relays` and hints are Phase-2
   retargeting.
3. Add a real no-match/release signal: a projection row, observer callback, or
   engine-owned timeout. Do not reference `event_claim_released` unless that
   projection is implemented.
4. Amend rung 2 with `crates/nmp-feed/src/types.rs`,
   `crates/nmp-nip01/src/timeline_projection/tests.rs`,
   `apps/chirp/nmp-app-chirp/tests/end_to_end.rs`, and all chirp-tui JSON
   fixtures.
5. Replace the LMDB-restored `timeline_authors` claim with current behavior:
   empty on kernel construction, repopulated by active-account kind:3 ingest or
   fresh-account `prepopulate_seed_contacts`.
6. Specify the account-change push path. If using a new projection event, list
   its producer and consumer in rung 1/rung 4. If using an existing handle, make
   sure the adapter gets a push notification, not a polling loop.
7. Rewrite L-2/L-5. Either add an event lookup/rebuild callback to the engine,
   or reduce the repost rules to what can be implemented from the event stream.
8. State `RootFeedSnapshot<C, A>` serialization bounds and add a compile-test or
   snapshot JSON test that proves `RootCard<TimelineEventCard,
   Nip10ReplyAttribution>` serializes.

## 6. Out-of-scope observations

- The current `claim_event` comments imply EOSE-driven release, but the
  projection refcount is host-release-driven. That mismatch predates this
  proposal and should be cleaned up separately if it confuses implementers.
- `LogicalInterest::SocialTimeline` may be unnecessary if the follow-set owner
  can register concrete interests directly. That would remove the most fragile
  new dependency surface.
- `timeline_authors` remains a substrate social cache. v3 is right to track that
  as follow-up debt, but the new adapter should not deepen its authority.
