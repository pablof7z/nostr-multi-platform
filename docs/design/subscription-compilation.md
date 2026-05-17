# Design: Subscription Compilation + Outbox Routing (M2)

> **Status:** Draft (M2 design + impl-prep).
> **Date:** 2026-05-18.
> **Companion docs:** [`docs/plan/m2-subscription-compilation.md`](../plan/m2-subscription-compilation.md); `docs/aim.md` §4.4–§4.5; `docs/design/ndk-applesauce-lessons.md` §7; ADR-0007 (diagnostic lanes); `docs/design/kernel-substrate.md` §3 (`ViewModule`) and §4 (`ActionModule`); `docs/product-spec/subsystems.md` §7.2–§7.3.
> **Scope:** Replace the "hardcoded two-role relay set" planner in `crates/nmp-core/src/kernel/{requests,ingest,mod}.rs` with a **subscription compilation stage** that turns logical interests into per-relay plans driven by NIP-65 mailboxes, and graduates outbox routing to a first-class planner subsystem. v1 is in-memory; M3 plugs it into LMDB. This is a design doc; no implementation lands in this PR.

This document is split into focused sub-files to stay under the 500 LOC ceiling (`AGENTS.md`).

- [Intro — problem statement + logical interest model](subscription-compilation/intro.md) (§1, §2)
- [Compiler — pipeline + plan-id contract + function migration table](subscription-compilation/compiler.md) (§3)
- [Recompilation — triggers and actor message shapes](subscription-compilation/recompilation.md) (§4)
- [Diagnostics — four-lane records + reverse-coverage view](subscription-compilation/diagnostics.md) (§5, §8)
- [nmp-nip65 — crate layout, traits, public surface](subscription-compilation/nip65.md) (§6)
- [Outbox — publish-planner seam + override action](subscription-compilation/outbox.md) (§7)
- [Tests — wire-frame audit gate](subscription-compilation/tests.md) (§9)

## Section map

| § | Topic | File |
|---|---|---|
| 1 | What is wrong with the current planner (cited line refs) | intro.md |
| 2 | Logical interest — formal definition + how views express them | intro.md |
| 3 | Compilation pipeline: authors → mailboxes → per-relay plans + plan-id | compiler.md |
| 4 | Recompilation triggers (kind:10002, view open/close, reconnect, account switch, manual, user-configured change) | recompilation.md |
| 5 | Four-lane diagnostic records (NIP-65 / hint / provenance / user-configured) | diagnostics.md |
| 6 | `nmp-nip65` file layout, `MailboxesViewModule`, public surface | nip65.md |
| 7 | `PublishPlanner` trait, write fan-out policy, override + debug warning | outbox.md |
| 8 | Reverse-relay-coverage diagnostic view ("this relay serves N authors of our timeline") | diagnostics.md |
| 9 | M2 exit-gate audit test path + assertions | tests.md |
| 10 | Open questions for follow-up ADRs | this file (below) |

## 10. Open questions

These remain to be resolved by ADRs after design review, not in this design pass.

1. **Plan-id stability under perturbation.** The intro/compiler picks "logical-interest set + author-mailbox snapshot ⇒ plan-id." That ties plan-id to mailbox membership, so a single new kind:10002 arrival reshuffles plan-ids for every interest including that author. An alternative scopes plan-id to the *logical-interest set only* and tracks per-relay assignment as a separate stable identifier. Pick one in an ADR; the test contracts in §9 assume the former.
2. **Filter-merge lattice formal-isation.** §3 step 3 lists which filter fields are safely mergeable across logical interests (`authors`, `kinds`, `since`, `until`, `limit`, tag operators). It does not yet specify the merge algebra for the corner case where two interests differ only in `limit` but agree on authors and kinds. Worth an ADR-level note before the lattice is coded; `docs/product-spec/subsystems.md` §7.2 promises "a formal merge lattice for `limit`, `since`, `until`, multi-filter arrays, and tag operators."
3. **Per-author indexer-fallback ledger row?** Today the compiler treats indexer fallback as an inline relay assignment. If the kind:10002 fetch is its own durable action (M6 ledger), the fallback becomes a tracked action with retry/cancel semantics. Cleaner for diagnostics; heavier for M2. Defer.
4. **Read-relay vs write-relay use for subscriptions on the same author.** NIP-65 defines write relays (outbox) for the author's own events and read relays (inbox) for events directed *at* the author. For a `Timeline { authors: [...] }` we want write relays. For a `Notifications { p: [author] }` we want inbox relays. The compiler distinguishes them by filter shape (`authors` vs `#p`), matching the `docs/product-spec/subsystems.md` §7.3 routing table. Document a corner case: kind-1 filtered by both `authors` and `#p` is rare but real (replies to the author from the author). Pick a precedence in an ADR; current bias is `authors` wins (write relays).
5. **User-configured relay precedence vs NIP-65.** A user adds `wss://my-private.example` to local config. Does it *augment* (union) or *override* (replace) NIP-65 routing for the active account? `subsystems.md` §7.3 default-resolves by NIP-65; user-configured is "fallback" in the indexer sense. ADR needs to spell out the augment/override question for the active account specifically.
6. **Auth-paused relays in compiled plans.** If a relay is in `RelayAuthState::ChallengeReceived`, the compiler still produces a plan that assigns interests to it (so reconnect-after-auth resumes correctly), but emission must pause. Is the pause modeled inside the compiler (per-relay gate) or inside the wire-emitter (consumes plans, applies pause)? Bias: wire-emitter, but the compiler must surface the pause as a fact for `LogicalInterestStatus`. Resolve before M5.
7. **NSE crate compilation surface.** `nmp-nip17-nse` (M9) runs in iOS Notification Service Extension with bounded memory; it needs to compile a single-author single-relay plan without the full planner. Confirm in an ADR that the compiler exposes a `compile_one(spec, mailbox_cache_snapshot) -> Plan` pure function suitable for NSE use, and that the function does not require a live actor.
