# NMP Framework Escape Hatches

The NMP framework guards the kernel behind doctrine seams (D0–D8). Most app
code should never need to cross these seams directly. This document catalogs
the **three escape hatches** — production-level lanes that callers can use to
reach below the framework guarantees — and explains when each is appropriate.

A capability the sound design cannot express through a typed seam is a design
gap to close, not an exception to whitelist. Use these only when the framework's
normal seams genuinely cannot serve your use case.

---

## Retired: the raw event tap

The original raw event tap (`nmp_app_register_raw_event_observer` /
`RawEventObserver`) has been **eliminated**. It had no backpressure, silently
matched all kinds on a null filter, and ran callbacks synchronously on the actor
thread. There is no replacement push sink — the speculative batched
`ExternalEventSink` C-ABI (register/ack/store-resync) that briefly stood in for
it has also been removed.

Two distinct needs the tap conflated are now served separately:

- **In-process relay forwarding** is an internal policy seam, not an escape
  hatch. The kernel owns an `ExternalEventSinkPolicy` dispatcher
  (`crates/nmp-core/src/substrate/external_event_sink/`) that fans verified
  inbound frames — including `Duplicate` outcomes with source-relay provenance —
  out to relay targets on a background worker thread. The only in-repo consumer
  is `IndexerRepublishPolicy`. It is not exposed over the FFI.
- **External per-event consumption** (e.g. the `hl` app's nostrdb mirror)
  reads through the store via the **bounded, backpressured pull cursor**
  (ADR-0058). The canonical contract is: register a `GlobalLog` cursor in
  `Protected { max_lag_entries }` mode → receive `nmp.pull.wake` →
  call `nmp_app_pull_page` → apply the page → persist `after_seq` →
  `AdvancePullCursor`. See `docs/architecture/external-consumers.md` for the
  full mirror consumption contract and the mirror-as-semantic-superset
  invariants (NIP-09 / NIP-40 applied by the mirror itself, never on
  retention evictions).

The lint rule `no_raw_tap_reintroduction` mechanically prevents both the
raw-tap symbols and the #1552-deleted native push C-ABI sink symbols
(`nmp_app_register_event_sink`, `retain_until_ack`, `event_sink_watermark`,
etc.) from reappearing.

---

## Retired: the snapshot projector

The schema-less JSON snapshot projector
(`nmp_app_register_snapshot_projection` / the generic `KernelSnapshot::projections`
map) has been **eliminated**. It was never wire-encoded — every host already
reconstructed its view from the typed FlatBuffers projection sidecar — so the
generic lane was dead producer code that bypassed D3 (typed projection routing).
Production host-rendered state now flows through a single canonical path:
`register_typed_snapshot_projection` (ADR-0037 typed runtime projections).

The lint rule `A6` mechanically prevents the JSON lane's symbols from
reappearing.

---

## 1. Action Module Seam — `NmpApp::register_action::<M>()`

**Module:** `crates/nmp-core/src/app.rs`  
**Rust API:** `NmpApp::register_action::<M>()` where `M: ActionModule`

This is **not** an escape hatch in the negative sense — it is the **preferred
way** to extend the kernel. An `ActionModule` provides:
- A typed action handler dispatched via `dispatch_action` JSON payloads.
- An optional typed `SnapshotProjector` for view delivery.
- An optional `LogicalInterest` set for subscription routing.

It is listed here because callers who reach for an ingest parser or inject
function often actually need an action module. If your use case involves (a)
triggering Nostr events from user input, or (b) projecting custom state into the
snapshot, use `ActionModule` before reaching for any escape hatch.

See `docs/dispatch-actions.md` for the action namespace catalog.

---

## 2. Test-Only Injectors — `nmp_app_inject_*`

**Module:** `crates/nmp-ffi/src/testing.rs`
**Gate:** `#[cfg(any(test, feature = "test-support"))]` — **never in production ABI**
**Symbols:** `nmp_app_inject_pre_verified_events`, `nmp_app_inject_signed_events`,
`nmp_app_inject_signed_event_json`

**What they give you:** Synthetic event injection into a live kernel for testing
— bypassing the relay-wire transport entirely and (for `inject_pre_verified_events`)
the Schnorr + id-hash verification gate.

**When appropriate:** Integration tests and REPL-driven diagnostics only.
Never call these from production app code; the `test-support` feature flag
prevents accidental inclusion. This is the only mechanically-gated kernel-bypass
exception.

---

## 3. IngestParser — `register_ingest_parser` / `replace_ingest_parser`

**Module:** `crates/nmp-core/src/kernel/ingest/mod.rs`  
**Rust API:** `NmpApp::register_ingest_parser` / `NmpApp::replace_ingest_parser`  
**C ABI:** none — Rust-only seam

**What it is:** A slot-keyed, sig-bearing inbound parse seam. Each parser is
registered under a `(kind, slot_name)` key; the kernel calls your closure for
every accepted event of that kind and passes the full sig-bearing JSON. On cold
start, cache-served replay fires the same closure path as live relay delivery —
so a single implementation handles both cases.

**What it gives you:**
- Full sig-bearing JSON (id + pubkey + created_at + kind + tags + content +
  **sig**) for each accepted event whose kind matches your registration.
- `Inserted` / `Replaced` / `Ephemeral` delivery signals so your parser can
  distinguish first-write from slot-lifecycle replace.
- Cache-served replay: when the kernel serves events from the local store on
  cold start, your parser fires identically to live ingest — no special-casing.
- Slot-keyed lifecycle: `replace_ingest_parser` atomically swaps the parser
  under an existing slot without tearing down and re-registering interest; use
  for identity-switch flows (e.g. NIP-17 DM inbox re-key on account change).

**Slot-uniqueness warning:** Each `(kind, slot_name)` pair must be globally
unique within the running app instance. Registering two parsers under the same
slot silently replaces the first. Use namespaced slot names (e.g.
`"nip17.dm_inbox"`) to avoid collisions.

**When appropriate:** Whenever you need to derive in-process state from
inbound events — decrypt gift-wraps, build projections, accumulate per-kind
views. This is the **preferred in-process consumption path**; an external store
mirror that needs verbatim signed frames uses the pull cursor (ADR-0058) —
see `docs/architecture/external-consumers.md`.

**What it bypasses:**
- D3 — your parser runs outside the kernel's typed projection dispatch; the
  parsed output must be stored and projected by your own code.

**Does NOT bypass:**
- D1 — the kernel only calls your parser for events that arrived via a wired
  subscription interest; you still need a matching `LogicalInterest` registered
  (typically via an `ActionModule` or a registered `LogicalInterest`).
- D8 — the parser closure runs inline on the ingest path; it must be cheap and
  non-blocking.

---

## Decision tree

```
Need to derive in-process state from inbound signed events (decrypt
gift-wraps, build projections, accumulate per-kind views)?
  → register_ingest_parser (#3) — fires on live ingest AND
    cache-served replay; supports slot-keyed lifecycle replace.

Need verbatim signed frames in an external store/relay-bridge mirror
(e.g. an out-of-tree nostrdb mirror)?
  → pull cursor (ADR-0058): GlobalLog cursor in Protected { max_lag_entries }
    mode → nmp.pull.wake → nmp_app_pull_page → apply page → persist
    after_seq → AdvancePullCursor.
    See docs/architecture/external-consumers.md for the full contract.

Need custom state in every snapshot?
  → typed sidecar via register_typed_snapshot_projection (ADR-0037),
    or an ActionModule snapshotProjector (#1)

Need to handle a dispatch_action payload or publish Nostr events?
  → ActionModule (#1)

Writing a test and need synthetic events without live relays?
  → test-only injectors (#2)
```
