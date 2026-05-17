# M4 — NIP-77 negentropy sync engine

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** Profile screen for a new author cold-syncs via NIP-77 against primal, visibly faster and with measured bytes savings vs REQ scan.

**Scope.** Per `product-spec.md` §7.8 and ADR (sync as engine, not feature):

**Subsystem deliverables.**

- `nmp-nip77` protocol module: negentropy reconciliation client (use `nostr-sdk`'s implementation or `negentropy` crate directly).
- Sync watermarks table active per-`(filter, relay)`.
- Planner consults watermarks before issuing historical REQ; sync-first backfill with REQ as fallback (when relay doesn't support NIP-77).
- Three built-in triggers: app foreground, view-open-with-gap, relay reconnect.
- `RunSync` manual action module.
- Per-relay NIP-77 capability negotiation (probe + cache result).
- Bytes-saved counter in diagnostics.

**Exit gate.**

- Cold open of a profile against primal: completes via negentropy, not REQ. Bytes-on-wire ≤ 5% of equivalent REQ on a 10k-event backfill.
- Cache-miss against a fully-synced `(filter, relay)` pair answers authoritatively (no fallback fetch).
- Relay reconnect after 10 min resumes from watermark; gap filled by sync.
- Mixed-capability test (one NIP-77 relay, one non-NIP-77): both populate the same store; non-NIP-77 falls back to REQ; bytes-saved diagnostic reflects the split.

**Runnable artifact.** iOS app with measurably faster profile cold-opens. Report in `docs/perf/m4/negentropy.md`.
