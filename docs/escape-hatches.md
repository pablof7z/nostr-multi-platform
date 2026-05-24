# NMP Framework Escape Hatches

The NMP framework guards the kernel behind doctrine seams (D0–D8). Most app
code should never need to cross these seams directly. This document catalogs
the **four escape hatches** — production-level lanes that callers can use to
reach below the framework guarantees — and explains when each is appropriate.

Use these only when the framework's normal seams genuinely cannot serve your
use case. Every escape hatch trades a framework guarantee for direct access.

---

## 1. Raw Event Tap — `nmp_app_register_raw_event_observer`

**Module:** `crates/nmp-core/src/ffi/raw_event_tap.rs`  
**Rust API:** `NmpApp::register_raw_event_observer`  
**C ABI:** `nmp_app_register_raw_event_observer` / `nmp_app_unregister_raw_event_observer`

**What it gives you:** The verbatim inbound `SignedEvent` JSON (id + pubkey +
created_at + kind + tags + content + **sig**) for every accepted event whose
kind matches your filter, delivered on a dedicated drain thread.

**What it bypasses:**
- D1 — subscription/planner routing is invisible; you receive events regardless
  of whether any subscription asked for them.
- D3 — no projection routing; you get the wire event, not a view object.
- D5 — outside the bounded snapshot cluster; high-volume kinds with a null
  filter will fire on every accepted event with no back-pressure.
- D8 — callback runs on the drain thread; any blocking operation stalls the
  drain.

**When appropriate:** Only when you need the `sig` field verbatim — e.g., MLS
transport that must forward a signed NIP-59 gift-wrap byte-for-byte. If you
only need the event content or a derived view, use the snapshot-projector seam
instead.

---

## 2. Snapshot Projector — `nmp_app_register_snapshot_projection`

**Module:** `crates/nmp-core/src/ffi/snapshot.rs`  
**Rust API:** `NmpApp::register_snapshot_projection`  
**C ABI:** `nmp_app_register_snapshot_projection`

**What it gives you:** A callback invoked on every snapshot tick. Your callback
returns a JSON string that is appended to `KernelSnapshot::projections` under a
host-chosen key, making custom app state visible to the host shell alongside
the kernel's built-in projections.

**What it bypasses:**
- D3 — your projector runs outside the kernel's typed projection system; the
  returned JSON is not validated against a schema and is appended verbatim.

**When appropriate:** When your app crate holds state that must be snapshot-
delivered to the host shell but that state is not owned by the kernel (e.g., a
NIP-29 group-chat projection in `nmp-nip29`). Prefer typed `SnapshotProjector`
trait implementations via the `ActionModule` registration path when possible.

**Important:** The projector callback runs on the actor thread inside the
snapshot tick — it MUST be cheap and non-blocking (D8).

---

## 3. Action Module Seam — `NmpApp::register_action::<M>()`

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

## 4. Test-Only Injectors — `nmp_app_inject_*`

**Module:** `crates/nmp-core/src/ffi/testing.rs`  
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

## Decision tree

```
Need the `sig` field verbatim?
  → raw event tap (#1)

Need custom state in every snapshot?
  → snapshot projector (#2) or ActionModule snapshotProjector (#3)

Need to handle a dispatch_action payload or publish Nostr events?
  → ActionModule (#3)

Writing a test and need synthetic events without live relays?
  → test-only injectors (#4)
```
