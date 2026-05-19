# Framework Magic §C6–§C7 — Outbox Routing

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/product-spec/subsystems.md` §7.3 (the resolution algorithm — read & write rows); `docs/design/subscription-compilation/outbox.md` (the `PublishPlanner` trait); `docs/design/subscription-compilation/compiler.md` §3 (the read-side compiler pipeline); `docs/design/ndk-applesauce-lessons.md` §9.5 (privacy-sensitive routes fail closed).

Both bullets in this chapter discharge cardinal doctrine **D3** ("outbox routing is automatic; manual relay selection is the opt-out") and `aim.md` §6 doctrine 5. C7 additionally discharges `aim.md` §6 doctrine 10 ("private events cannot be accidentally republished to public relays") and `product-spec/overview-and-dx.md` §3.3 bug #4.

## C6. Read fan-out: `authors`-filter subscriptions go to those authors' write relays, de-duplicated

**Statement.** Any subscription whose canonical filter has a non-empty `authors` set is compiled into one wire REQ per relay in the **union of those authors' write relays** (kind:10002), with each per-relay REQ carrying only the authors that declared that relay. Authors with unknown mailboxes are routed to the configured indexer set as fallback; once their kind:10002 lands, the planner recompiles and the authors migrate to their declared relays.

**Framework does:** the compilation pipeline at `docs/design/subscription-compilation/compiler.md` §3 — Stages 1 (resolve mailboxes), 2 (assign per-relay author subsets), 3 (merge sub-shapes), 4 (emit per-relay REQs). The indexer-fallback path is `RoutingSource::Indexer`; the post-NIP-65-arrival migration is `Trigger::Nip65Arrived` per `docs/design/subscription-compilation/recompilation.md` §4.2. The mailbox cache is read from the `MailboxCache` trait defined in `nmp-nip65` (`docs/design/subscription-compilation/nip65.md`).

**App writes:** nothing. The view spec names authors (or, for follow-derived views, names nothing and the view module reads the active account's follow-set — see C5). The app never names a relay URL on a read path.

**Failure mode prevented:** the bug `ndk-applesauce-lessons.md` §3 names: *"NDK's convenience can blur boundaries"* combined with the bug `product-spec/subsystems.md` §7.3 lines 89–90 names: *"Posts to relays the author hasn't declared as write relays."* On the read side, the symmetric failure is reading from the global content relay and missing an author's actual events because the author publishes only to their own write relay. The structural enforcement is that the view spec has no relay field; the only API surface that names a relay is the explicit override (named, audited, one-shot per `docs/design/subscription-compilation/outbox.md` §7.4).

**Test:** `c6_authors_subscription_routes_to_per_author_write_relays`. This test is a **rename of and dependency on** the M2 audit gate test `timeline_compiles_to_per_relay_union` (`docs/design/subscription-compilation/tests.md` §9.2 assertion 2). The framework-magic version asserts the same observable but accesses the data through the **public view path**, not the planner harness:

1. Pre-seed mailbox cache with 1000 authors using three overlapping relay sets (per the M2 test).
2. Open `TimelineView { authors: <1000 pubkeys>, kinds: [1, 6] }` through the actor's public dispatch surface.
3. Read the wire-emission audit log (exposed via `DebugDiagnostics`) and assert: relay count = union; per-relay author partition = subset semantics; sub-shape merge = one REQ per relay; plan-id stable on re-compile.
4. Ingest a new kind:10002 for one author moving them off relay-1 onto relay-4; assert exactly one CLOSE-and-REQ pair fires for the affected slice; no churn for the unmoved authors.
5. **NDK comparison for step 4:** NDK's `refreshRelayConnections` (`core/src/ndk/index.ts:458-471`, `subscription/index.ts:787-812`) only *adds* relays and never removes stale ones. NMP's wire-emitter diff emits CLOSE for the stale slice and a new REQ for the author's updated relay, covering the window with `since: last_seen_for_author`. The test asserts no events are missed during the transition.

The "via the public view path" framing matters: M2's test exercises the compiler directly; the framework-magic test exercises the contract surface (open a view, watch the wire). Both must pass.

**Milestone owner:** **[PENDING M2]**. Test checked in as `#[ignore = "pending M2 compiler + view bridge"]`. Removed in the M2 framework-magic delta.

## C7. Write fan-out: outbox + recipient-inbox; private events fail closed

**Statement.** Every publish action's signed event is routed by the `PublishPlanner` (`docs/design/subscription-compilation/outbox.md` §7.1) according to a `PublishPrivacy` mode the action declares. **Public** events go to author write relays. Discovery kinds (kind:0, kind:3, kind:1xxxx) additionally fan out to user-configured indexer relays so the author remains discoverable. **PublicWithNotifications** events go to author writes ∪ recipient inboxes (`#p` tagged pubkeys). **PrivateToRecipients** events (gift-wrapped per NIP-59) go to **only** resolved recipient inbox relays — never the author's writes, never the active session's defaults, never the indexer set. If any recipient has no declared inbox, the publish fails closed with `PublishPlanError::PrivateRecipientUnroutable`.

**Framework does:** the algorithm at `docs/design/subscription-compilation/outbox.md` §7.3 (write fan-out, all 6 numbered steps), specifically:

- Step 2 forbids indexer fallback for any write path (`NoAuthorRelays` returned instead).
- Step 3(b)'s `Indexer` check on recipient inbox lookups is the structural fail-closed for private events.
- The `PublishWithOverride` action is the *only* `AppAction` variant carrying a `Vec<RelayUrl>` field, and it is forbidden from widening a `PrivateToRecipients` plan to public relays (`outbox.md` §7.4 rule 4).

**App writes:** nothing — for the publish path. The app dispatches a publish action (`SendNote`, `React`, `SendDm`, etc.); the action's privacy mode is determined by the action type, not by an app-supplied parameter. There is no `relays` field on `SendNote`. The override exists for tests, migrations, and operator power-user flows; it is structurally outside the safe app path.

**Failure mode prevented:** §3.3 bug #3 ("Publish of an event to relays the author has not declared as write relays") and bug #4 ("DM published to public relays"). Plus the doctrine-10 footgun: a "send everywhere" fallback that publishes a gift wrap to the global content relay because the recipient's inbox lookup returned empty.

**Test:** `c7_publish_routes_outbox_and_private_fails_closed`. The test has three sub-paths:

1. **Public:** seed Alice's mailbox with two write relays; dispatch a public `SendNote` action; assert the resulting publish plan has exactly those two relays and no others, and that `required_success_count = max(1, ceil(2/3)) = 1` per `outbox.md` §7.3 step 3(a).
2. **PublicWithNotifications:** dispatch a note tagging Bob (Bob has one inbox relay seeded); assert the plan is Alice's writes ∪ Bob's inbox, with the correct `PublishRouteReason::AuthorWriteRelay` / `RecipientInbox` tagging per assignment.
3. **PrivateToRecipients (fail-closed):** dispatch a (post-M9, but the planner shape is testable in isolation today) gift-wrap to Charlie, who has **no kind:10002**. Assert the publish plan errors with `PublishPlanError::PrivateRecipientUnroutable { recipient: charlie }` and that **no wire EVENT frame is emitted on any relay** — checked by reading the relay worker's outbound audit log.
4. **Override rejection:** dispatch a `PublishWithOverride` carrying a `PrivateToRecipients` inner action and an override relay set that includes a non-inbox URL; assert it rejects with `PublishPlanError::OverrideRejected { reason: "private widen" }` (rule 4 of `outbox.md` §7.4).
5. **Override audit:** dispatch a `PublishWithOverride` on a public action; assert the side-effect lane emits `Diagnostic::PublishOverrideUsed { ... }` and the debug log line per `outbox.md` §7.4 (3).

**Milestone owner:** **[PENDING M2 seam → M6 publish]**. M2 lands the `PublishPlanner` trait + `Nip65PublishPlanner` + the `PublishWithOverride` action (`docs/design/subscription-compilation/outbox.md` §7.1, §7.2, §7.4). M6 lands `SendNoteAction` as the first concrete consumer. Test checked in as `#[ignore = "pending M2 planner + M6 first consumer"]`. Sub-paths 3 and 4 of the test exercise the planner in isolation (M2-completable); 1, 2, and 5 require M6's action consumer.

## The two bullets together discharge D3

C6 covers the read side; C7 covers the write side. Together they discharge cardinal doctrine D3 in full — every relay-touching operation routes through framework policy, and the only API surfaces that name a relay URL are:

- the `PublishWithOverride` action (write path, audited);
- the planner's diagnostic accessors (read-only);
- the user-configured-relays settings surface (configuration, not per-operation).

The app's domain code, view modules, and action modules **never** name a relay. That is the doctrine-D3 boundary the contract holds in place.

## What this chapter does not cover

- The publish-fail retry/back-off policy — that's M6 territory (`docs/design/subscription-compilation/outbox.md` §7.6 deferred items). The contract's fail-closed guarantee is structural (the wire frame is never emitted), not about how long the system retries before giving up.
- The action ledger row schema — `docs/design/kernel-substrate.md` §4 owns it. C7 cares that the ledger correlates the per-relay attempts; the contract does not specify the row layout.
- NIP-77 sync routing — that's C10 in `sync.md`. Sync and live REQ should share relay policy (`ndk-applesauce-lessons.md` §6 last paragraph), but the symmetric assertion lives with C10.
- NIP-42 auth-paused publishes — M5. The override action does not unblock an auth-paused relay; auth pause is a wire-emitter gate, not a planner decision (`docs/design/subscription-compilation/recompilation.md` §4.2 trigger A9 open question).
