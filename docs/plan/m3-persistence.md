# M3 — Persistence (LMDB) + full insert invariants

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** iOS app cold-starts in ≤ 1.5 s with the previous session's events already on screen.

**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.

**Subsystem deliverables.**

- LMDB schema design doc (`docs/design/lmdb-schema.md`) — key encoding, secondary indexes, tombstones, watermarks table (populated in [M4](m4-negentropy.md)), backup/export format.
- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
- Migration plumbing (ties into `DomainModule::migrations()`).
- GC working set policy per ADR-0003: hot ≤ 10k events resident + claim-pinned set; cold on disk.

**Exit gate.**

- Cold-start with primed LMDB: time-to-first-painted-timeline ≤ 1.5 s on iPhone 12.
- Working-set memory under sustained scroll: ≤ 100 MB at 100 active views / 10k hot events / 1 M cached on disk.
- Replaceable correctness across restart: a kind:0 written, app killed, app reopened — the latest version is served, not stale.
- Kind:5 self-delete persists; foreign kind:5 ignored.

**Runnable artifact.** iOS app surviving termination + relaunch with state preserved. Report in `docs/perf/m3/persistence.md`.
