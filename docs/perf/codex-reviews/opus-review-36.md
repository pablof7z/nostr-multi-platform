# Opus Direction Review #36 — The merge pipe works; the consumer pipe doesn't

Date: 2026-05-21
Reviewer: Opus (architectural direction audit)
Scope: NMP kernel + `dispatch_action` seam + Chirp FFI + the NIP-57 / NIP-29 / async-capability triad. Ground truth: `git log master`, file-reads, `git cherry`.

---

## TL;DR — read this first

- **Review #35's "broken merge pipe" panic is resolved and was partly an artifact of stale local git.** PRs #99–#105 are all on `master` today: `HttpCapability` (ADR-0023), snake_case NIP-29 namespaces, `last_action_result` scalar deleted, `actor_queue_depth`, and ADR-0024. The merge pipe works. Stop re-litigating it.
- **The new pathology is the opposite one: a working merge pipe shipping inert features.** Three consecutive cycles landed registration-side infrastructure — `nmp.zap`, 15 `nip29.*` namespaces, `HttpCapability` — with **zero user-reaching consumers.** Green CI, merged, on master, and a Chirp user can do nothing new.
- **`nmp.zap` is functionally inert, not just "incomplete."** A Swift caller that invokes `nmp.zap` today gets a 32-hex `correlation_id` and **no satoshi moves.** The executor publishes the kind:9734 to Nostr relays and stops; the LNURL POST → bolt11 → wallet legs do not exist. The transport that would enable them (`HttpCapability`) has no executor wired to it, and the async protocol that would let an executor use it (ADR-0024) is **0 of 5 checklist items done.**
- **ADR-0024 is a decision record with an unstarted implementation.** No `ActorCommand::CapabilityResultReady`, no `nmp_app_deliver_capability_result`, no `ActionModule::resume`. The 24th ADR on a project whose users can do 4 things, and its own checklist gates the feature it was written for.
- **Highest-leverage next move: build ONE NIP-29 group-chat read screen.** It is the only candidate that reaches a user without first implementing a 5-item async protocol. Make it the forcing function for the 15 dormant `nip29.*` executors — or delete 14 of them. The lukewarm "keep them, they're cheap" answer has now survived ~5 reviews and that is the problem.

---

## The shape of the problem

Reviews #25–#34 kept coining variants of "shipped-but-inert." Review #35 escalated to "not shipped at all" — but that was reasoning against stale local git; the work *was* on origin. With that cleared, the real and durable pathology is now in focus and it is not about merges:

**The project reliably builds the registration side of a seam, merges it green, and calls it shipped. It does not build the consumption side.**

Concretely, on `master` right now:

| Surface | Registration side | Consumption side | User impact |
|---|---|---|---|
| `nmp.publish` | ✅ executor + module | ✅ Swift `KernelBridge.publishNote` | **Real** — user posts a note |
| `chirp.react/follow/unfollow` | ✅ | ✅ Swift callers | **Real** |
| `nmp.zap` | ✅ executor + module | ❌ zero Swift callers; executor publishes 9734 then stops | **None** — no payment occurs |
| 15× `nip29.*` | ✅ executors + modules, snake_case, tested | ❌ zero Swift callers, zero screens | **None** |
| `HttpCapability` | ✅ Rust seam + iOS `HttpCapability.swift` (269 LoC) | ❌ no executor calls `HttpCapabilityWiring` | **None** — kernel still makes zero HTTP calls in practice |
| ADR-0024 async capability | ✅ decision record | ❌ 0/5 checklist items | **None** |

Four live user-reaching dispatches. Six merged-but-inert surfaces. The ratio is the diagnosis.

---

## 1. Stop / Start / Continue

### STOP

- **Stop registering new namespaces or capabilities until an existing dormant one has a Swift caller.** `nmp.zap`, the 15 `nip29.*`, and `HttpCapability` are all registered-but-uncalled. A 16th registration, a 3rd `CapabilityModule`, or a 2nd inert NIP executor cluster makes the green-CI-vs-user-reality gap wider, not the product better. This is a hard freeze, not a guideline.
- **Stop writing ADRs whose implementation checklists outlive the next ADR.** ADR-0024 has a 5-item "Required before `ZapModule` can land" checklist; 0 are done, and `ZapModule` landed anyway (PR #98) in violation of the ADR's own gate. Decisions are cheap; ADR-0024 is the 24th. The scarce resource is a merged consumer, not a 25th decision record.
- **Stop calling `feat(nip57)` / `feat(nip29)` on commits that add an inert executor.** The honest verb for a registered-but-uncalled namespace is `scaffold`. `feat` in the changelog is what lets the heartbeat narrative report inert surfaces as delivered value — exactly the drift reviews #33–#35 kept tripping over.

### START

- **Start with the consumer.** The next feature PR must begin in Swift — a screen, a button, a list — and pull the Rust side toward it. The project has the inverse habit (Rust seam first, Swift "later") and "later" has not arrived for `nmp.zap` or any `nip29.*`.
- **Start a "does a user see this?" gate per cycle.** A cycle that ends with new registered namespaces and no new Swift dispatch call site is a failed cycle, the same way review #35 wanted a merge gate. The merge gate is now satisfied automatically; this is the gate that isn't.

### CONTINUE

- The single-actor kernel boundary and the `dispatch_action` seam for the 4 live namespaces. This is the project's real asset and the one path proven end-to-end. The D7 `created_at == 0` re-stamp in `dispatch.rs:323` is exactly right — keep that discipline.
- Doctrine-lint (D0/D6/D7/D8). The kernel architecture is genuinely clean; that has never been the problem.
- The snake_case namespace uniformity now on master (`nip29.create_group`, `nip29.post_chat_message`, …). Review #34's CamelCase wire bug is fixed; do not regress it.

---

## 2. Smallest user-visible win

**The two honest candidates have very different costs. Do not smear them.**

**Candidate A — a NIP-29 group-chat READ screen. Cost: one screen, zero async, no new Rust.**
A new projection that subscribes to a group's `h`-tagged events (kinds 9, 11, 1111) and renders them in a list. This is the same shape as the existing `ModularTimelineProjection` already wired through `nmp_app_chirp_register`/`nmp_app_chirp_snapshot`. It needs:
- a group-scoped projection in `nmp-nip29` (or a `ModularTimelineSpec` with an `h`-tag filter),
- a Swift view that calls the existing snapshot FFI.
No posting, no async capability, no ADR-0024. A user opens a group and sees messages. This is the smallest delta that renders something new on screen.

**Candidate B — a complete NIP-29 post. Cost: one screen + nothing else, because `nip29.post_chat_message` already has a live executor.**
`PostChatMessageAction` is registered and its executor emits `PublishUnsignedEventToRelays` pinned to the group's host relay. The *only* missing piece is a Swift `dispatchAction("nip29.post_chat_message", …)` call site behind a text field. This is the smallest win that also *proves the NIP-crate executor path end-to-end with a real user*.

**Recommendation: Candidate B, paired with Candidate A's read view.** One screen — read the group + post to it — lights up the NIP-29 cluster's first real caller and is achievable in a single cycle. Files: a new `GroupChatView.swift` + `GroupChatStore.swift` in `ios/Chirp`, a group-events projection in `nmp-nip29`, and a `dispatchAction` call site in `KernelBridge.swift`. Zero new Rust *registration*; this is pure consumption work.

**Explicitly NOT the smallest win: a working zap.** It requires all 5 ADR-0024 checklist items first. Anyone proposing "zap" as a quick win is mistaking a `correlation_id` for a payment.

---

## 3. Architecture debt vs. new features — the ratio

**Next 3 PRs: 1 cleanup, 2 consumer-features. But the "features" must be consumption-only, not new registration.**

The disease is registration-without-consumption. The cure is not more debt-cleanup (the kernel is clean — doctrine-lint is at 0 findings, and there is no structural rot to chase). The cure is *forcing the existing inert surfaces to either light up or be deleted*.

- **PR 1 (cleanup / forcing):** Resolve the NIP-29 cluster — see §5. Either build the screen or delete 14 executors. This is "cleanup" in the sense of closing the registered-but-uncalled gap.
- **PR 2 (consumer-feature):** The NIP-29 group-chat read+post screen (§2). New Swift, no new Rust registration.
- **PR 3 (consumer-feature OR async groundwork):** Either a second NIP-29 screen, or — if zaps are deemed the priority — start ADR-0024 item 1 (`ActorCommand::CapabilityResultReady`). Not both.

The ratio that matters is not debt:features. It is **registration:consumption — and it must be 0:N.** Zero new namespaces, zero new capabilities, zero new ADRs for the next 3 PRs.

---

## 4. The async-capability question (ADR-0024)

**The implementation gap is total: 0 of 5 checklist items done.** Verified on master — none of these exist:
- `ActorCommand::CapabilityResultReady { correlation_id, result_json }` — absent.
- C-ABI `nmp_app_deliver_capability_result` — absent.
- `ActionModule::resume()` (or equivalent re-entry seam) — absent.
- Swift `URLSession` completion → `nmp_app_deliver_capability_result` — absent.
- `ZapModule` state machine (`Idle → AwaitingLnurlInfo → AwaitingInvoice → Done`) — absent.

The ADR is sound — fire-and-forget + C-ABI re-entry is the right design. But `ZapModule` shipped *before* it in violation of the ADR's own gating language. ADR-0024 currently functions as a paper alibi for `nmp.zap`'s inertness rather than a plan being executed.

**Minimum viable first consumer: NOT `ZapModule`.** A full zap is a two-hop saga (LNURL GET → LNURL POST) — the worst possible first exercise for a brand-new async protocol. The minimum viable first consumer is a **single-hop** async capability:

- **NIP-05 verification** (`GET https://domain/.well-known/nostr.json?name=…`) — one GET, parse, done. Exercises the entire ADR-0024 machinery with no saga state machine, and produces a visible result (a verified badge on a profile).
- **A link-preview / kind:0 image fetch** — similar single-hop shape.

Build the async protocol against a one-hop consumer first. Only once `CapabilityResultReady` round-trips cleanly should `ZapModule`'s two-hop saga be attempted.

**But:** async-capability is not the highest-leverage *next* thing. The NIP-29 screen (§2) delivers a user-visible win in one cycle with zero async. Sequence: NIP-29 screen first, async-capability groundwork second.

---

## 5. The NIP-29 question — 15 executors, 0 callers

**This answer has been "build one screen / keep them, they're cheap" for ~5 reviews and nothing happened. #36 forces the fork.**

**DO THIS: build the group-chat screen this cycle (§2, Candidate B).** It needs exactly one executor — `nip29.post_chat_message` — plus a read projection. Building it converts one of the 15 from inert to live and proves the NIP-crate executor path with a real user.

**Then: delete the executors that the screen does not reach, on a deadline.** After the screen lands, `post_chat_message`, `react_in_group`, and `comment_in_group` are plausibly reachable from a chat UI. The admin cluster and `post_artifact` / `post_discussion` / `share_event_into_group` / `join_request` / `leave_request` are not reachable from a chat screen and have no roadmap to a screen. **If a `nip29.*` executor has no Swift caller within 2 cycles of this review, delete it.** It is macro-generated; re-deriving it is a `wire_action!` line.

The verdict is not "delete all" and not "keep all." It is: **build one screen now, then delete every executor that screen plus its immediate successor do not consume, on a 2-cycle clock.**

---

## 6. What NMP should NOT do

- **Should not ship a `ZapModule` UI before ADR-0024 is implemented.** A zap button on master today would dispatch `nmp.zap`, return a `correlation_id`, publish a kind:9734 to relays, and move zero satoshis. Shipping that button is shipping a lie to the user.
- **Should not write ADR-0025+ until ADR-0024's checklist is done.** The project has 24 decision records and a 4-verb product. A moratorium on new ADRs until ADR-0024 is closed would cost nothing real and would force the implementation.
- **Should not add a third `CapabilityModule` (or expand `HttpMethod`) before `HttpCapability` has one executor actually calling `HttpCapabilityWiring`.** Today `HttpCapability` is a fully-built, iOS-implemented seam that no Rust executor invokes. Building a third repeats the registration-without-consumption pattern at the capability layer.
- **Should not run a wide parallel-agent fan-out for feature work right now.** The next 3 PRs are a tightly-coupled consumer story (screen → executor → projection). Fan-out manufactures merge conflicts and, worse, manufactures more inert registration branches.

---

## Appendix A — verification commands run

```
git merge-base --is-ancestor <sha> master   # PRs #99-#105: all ON master
git cherry origin/master origin/worktree-agent-*   # ADR-0024, snake_case: "-" (equiv on master)
grep -rn "CapabilityResultReady|deliver_capability_result|fn resume" crates/   # 0 hits
grep -rln "HttpCapabilityWiring" crates/ --exclude substrate/http.rs   # 0 executor callers
grep -rln "nmp.zap|nip29\." ios/ apps/ --include=*.swift   # 0 Swift callers
grep -rln "9734|bolt11" crates/nmp-core/src   # only generic publish + NIP-47 wallet
```

## Appendix B — the 44 orphan `worktree-agent-*` branches

44 `worktree-agent-*` branches are 1–3 commits ahead of `origin/master`. Spot-checked via `git cherry`: ADR-0024 and the snake_case fix show `-` (an equivalent patch is already on master). These are **stale housekeeping, not lost work** — review #35's "stranded work" finding does not reproduce. Recommend a one-time `git push origin --delete` sweep of branches whose tip is `git cherry`-equivalent to master.

---

## The single highest-leverage action

**Build one NIP-29 group-chat screen — read the group, post to it — consuming `nip29.post_chat_message`.** It is the only move that turns an inert surface into a user-visible feature in one cycle, with zero new registration and zero async. It proves the NIP-crate executor path with a real user, and it sets the 2-cycle clock under which the other 14 `nip29.*` executors either get a caller or get deleted.
