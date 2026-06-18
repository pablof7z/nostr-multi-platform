# NMP Framework Escape Hatches

The NMP framework guards the kernel behind doctrine seams (D0–D8). Most app
code should never need to cross these seams directly. This document catalogs
the **four escape hatches** — production-level lanes that callers can use to
reach below the framework guarantees — and explains when each is appropriate.

Use these only when the framework's normal seams genuinely cannot serve your
use case. Every escape hatch trades a framework guarantee for direct access.

---

## 1. Raw Event Tap — `nmp_app_register_raw_event_observer`

**Module:** `crates/nmp-ffi/src/raw_event_tap.rs`
**Rust API:** `NmpApp::register_raw_event_observer`
**C ABI:** `nmp_app_register_raw_event_observer` / `nmp_app_unregister_raw_event_observer`

**What it gives you:** The verbatim inbound `SignedEvent` JSON (id + pubkey +
created_at + kind + tags + content + **sig**) for every accepted live-ingest
event whose kind matches your filter, delivered on a dedicated drain thread.

**Important: live ingest only.** The tap fires on live relay delivery
(including `Duplicate` outcomes). It does **not** fire on cache-served replay.
If you need your consumer to see both live events and events served from the
local store on cold start, use `register_ingest_parser` (rule A5 / escape hatch #4) instead.

**What it bypasses:**
- D1 — subscription/planner routing is invisible; you receive events regardless
  of whether any subscription asked for them.
- D3 — no projection routing; you get the wire event, not a view object.
- D5 — outside the bounded snapshot cluster; high-volume kinds with a null
  filter will fire on every accepted event with no back-pressure.
- D8 — callback runs on the drain thread; any blocking operation stalls the
  drain.

**When appropriate:** Only when you need the `sig` field verbatim **to
forward the exact signed frame to an external store or relay bridge** — e.g.
the `hl` app's nostrdb mirror that stores events locally including their
signatures. If you need to derive in-process state or projections, use
`register_ingest_parser` (rule A5) instead.

**Raw-tap retirement ladder (four-PR history):**
- PR-1 (#1137) — NIP-17 DM inbox moved from raw tap to `IngestParser`; cache-serve replay wired.
- PR-2 (#1145) — Marmot moved from raw tap to `IngestParser`; slot-keyed replace semantics added.
- PR-3 (#1148) — chirp-tui debug raw-event cache moved to `IngestParser`.
- PR-4 (this PR) — tap narrowed to verbatim-forwarding contract; `swap_dm_inbox_observer` dead surface deleted; lint backstop (rule A5) added.


---

## 2. Action Module Seam — `NmpApp::register_action::<M>()`

**Module:** `crates/nmp-core/src/app.rs`  
**Rust API:** `NmpApp::register_action::<M>()` where `M: ActionModule`

This is **not** an escape hatch in the negative sense — it is the **preferred
way** to extend the kernel. An `ActionModule` provides:
- A typed action handler dispatched via `dispatch_action` JSON payloads.
- An optional `SnapshotProjector` for view delivery.
- An optional `LogicalInterest` set for subscription routing.

It is listed here because callers who reach for a raw tap or inject function
often actually need an action module. If your use case involves (a) triggering
Nostr events from user input, or (b) projecting custom state into the snapshot,
use `ActionModule` before reaching for any escape hatch.

See `docs/dispatch-actions.md` for the action namespace catalog.

---

## 3. Test-Only Injectors — `nmp_app_inject_*`

**Module:** `crates/nmp-ffi/src/testing.rs`
**Gate:** `#[cfg(any(test, feature = "test-support"))]` — **never in production ABI**
**Symbols:** `nmp_app_inject_pre_verified_events`, `nmp_app_inject_signed_events`,
`nmp_app_inject_signed_event_json`

**What they give you:** Synthetic event injection into a live kernel for testing
— bypassing the relay-wire transport entirely and (for `inject_pre_verified_events`)
the Schnorr + id-hash verification gate.

**When appropriate:** Integration tests and REPL-driven diagnostics only.
Never call these from production app code; the `test-support` feature flag
prevents accidental inclusion.

---

## 4. IngestParser — `register_ingest_parser` / `replace_ingest_parser`

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
views, or feed an external projection that must also see cached events on cold
start. This is the **preferred path over the raw event tap** for any consumer
that does more than forward a verbatim signed frame.

**What it bypasses:**
- D3 — your parser runs outside the kernel's typed projection dispatch; the
  parsed output must be stored and projected by your own code.

**Does NOT bypass:**
- D1 — the kernel only calls your parser for events that arrived via a wired
  subscription interest; you still need a matching `LogicalInterest` registered
  (typically via an `ActionModule` or a `register_raw_event_observer` interest).
- D8 — the parser closure runs inline on the ingest path; it must be cheap and
  non-blocking.

---

## Decision tree

```
Need the `sig` field to forward the signed frame verbatim to an
external store or relay bridge (e.g. nostrdb mirror)?
  → raw event tap (#1)
  NOTE: live ingest only — does NOT see cache-served replay.

Need the `sig` field to derive in-process state (decrypt gift-wraps,
build projections, accumulate per-kind views)?
  → register_ingest_parser (rule A5) — fires on live ingest AND
    cache-served replay; supports slot-keyed lifecycle replace.

Need custom state in every snapshot?
  → ActionModule snapshotProjector (#2)

Need to handle a dispatch_action payload or publish Nostr events?
  → ActionModule (#2)

Writing a test and need synthetic events without live relays?
  → test-only injectors (#3)
```
