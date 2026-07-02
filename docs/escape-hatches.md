# NMP Framework Escape Hatches

The NMP framework guards the kernel behind doctrine seams (D0–D8). Most app
code should never need to cross these seams directly. This document catalogs
the current extension and test escape seams and explains when each is
appropriate.

A capability the sound design cannot express through a typed seam is a design
gap to close, not an exception to whitelist. Use these only when the framework's
normal seams genuinely cannot serve your use case.

## 1. Action Module Seam — `NmpApp::register_action::<M>()`

**Module:** `crates/nmp-core/src/app.rs`  
**Rust API:** `NmpApp::register_action::<M>()` where `M: ActionModule`

This is **not** an escape hatch in the negative sense — it is the **preferred
way** to extend the kernel. An `ActionModule` provides:
- A typed action handler dispatched through the ADR-0071 byte doorway.
- Optional typed projection state for view delivery.
- An optional `LogicalInterest` set for subscription routing.

It is listed here because callers who reach for an ingest parser or inject
function often actually need an action module. If your use case involves (a)
triggering Nostr events from user input, or (b) projecting custom state into the
snapshot, use `ActionModule` before reaching for any escape hatch.

## 2. Test-Only Injectors — signed-event seeding

**Modules:** `crates/nmp-core/src/testing.rs`, `crates/nmp-native-runtime/src/testing.rs`
**Gate:** `#[cfg(any(test, feature = "test-support"))]` — **never in production ABI**
**Symbols:** `nmp_core::testing::inject_signed_events`,
`NmpApp::inject_signed_event_json_for_test`

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
mirror that needs verbatim signed frames uses the pull cursor (ADR-0072) —
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
  → pull cursor (ADR-0072): GlobalLog cursor in Protected { max_lag_entries }
    mode → nmp.pull.wake → NmpApp::mirror_pull_page → apply page → persist
    after_seq → AdvancePullCursor.
    See docs/architecture/external-consumers.md for the full contract.

Need custom state in every snapshot?
  → typed sidecar via register_typed_snapshot_projection (ADR-0072)

Need to handle typed write input or publish Nostr events?
  → ActionModule (#1)

Writing a test and need synthetic events without live relays?
  → test-only injectors (#2)
```
