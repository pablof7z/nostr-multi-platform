# M2 — Subscription compilation + outbox routing

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** Same iOS app as [M1](m1-twitter-slice.md), but timeline subscriptions are routed per-author to those authors' write relays per NIP-65, not to the hardcoded primal/purplepag.es pair. Diagnostics screen visibly shows the per-relay fan-out and which authors each relay covers.

**Scope.** The planner becomes a **subscription compilation stage** per the NDK/Applesauce lessons doc. Logical interests get compiled into per-relay plans; recompilation happens when relay metadata arrives late. NIP-65 routing is the default for both reads and writes. Provenance / NIP-65 / relay hints / user-configured relays are four distinct facts that inform each other but never collapse.

**Subsystem deliverables.**

- `nmp-nip65` protocol module: Mailboxes view module (parsed kind:10002); outbox routing as a planner subsystem; recompilation triggers (kind:10002 arrival, view open, relay reconnect).
- Planner refactored from "hardcoded relay set" to "compiler of logical interests → per-relay plans." See `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons).
- Per-pubkey relay-list cache (durable, even before LMDB lands — keep it in-memory until [M3](m3-persistence.md), but the data model is correct).
- Indexer fallback when a pubkey's kind:10002 is unknown: opportunistic discovery from a configurable indexer relay set.
- Reverse-relay-coverage view for diagnostics: "this relay is serving N authors of our timeline."

**Exit gate.**

- Bug-extinction test #3 (publish to wrong relays): no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning.
- Subscription compilation correctness: for a timeline of 1000 authors, the compiled plan opens REQs only against the union of those authors' write relays (de-duplicated). Test asserts on the wire frame count.
- Late-arriving kind:10002 triggers recompilation: an author whose mailbox was unknown gets re-routed once their kind:10002 lands, without the platform observing protocol churn.
- Distinct-source visibility: the diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately.

**Runnable artifact.** iOS app with measurably different relay-fan-out behavior; demo screenshot in `docs/perf/m2/outbox-routing.md` showing per-relay coverage.
