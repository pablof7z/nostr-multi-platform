# 24 — Reference cards

Bookmarkable single-page tables. Every entry verified at master tip. Where a
row is a long-term catalog entry not yet shipped, it carries an explicit
status marker; unmarked rows ship today.

> The kernel enums below (`KernelAction`/`KernelUpdate`/`KernelViewSpec`) are
> the *generic kernel surface* in `crates/nmp-core/src/app.rs`. Per-app
> `AppAction`/`AppUpdate`/`ViewSpec` are **codegen output** (`nmp gen
> modules`) that wrap these plus protocol/app-core variants — see §15.

## Card 1 — Kernel surface (today, `crates/nmp-core/src/app.rs:1-30`)

| `KernelAction` | `KernelUpdate` | `KernelViewSpec` |
|---|---|---|
| `Start` | `Started { rev }` | `Diagnostics` |
| `Stop` | `Stopped { rev }` | |
| `OpenView { namespace, key }` | `ViewOpened { namespace, key }` | |
| `CloseView { namespace, key }` | `ViewClosed { namespace, key }` | |
| `RunDiagnostics` | `Diagnostics { summary }` | |

`AppState = { rev: u64, open_view_count: usize }`. Every update carries a
monotonic `rev`; platforms drop updates with `rev` ≤ last seen.

## Card 2 — Extension seams + traits (`crates/nmp-core/src/substrate/`, `crates/nmp-ffi/src/lib.rs`)

| Seam / trait | Owns | One-liner | Source |
|---|---|---|---|
| `register_action<M>()` | write path | `start()` validates, `execute()` enqueues `ActorCommand` | `action.rs:56` |
| `register_snapshot_projection(key, fn)` | read output | JSON slice pushed under `projections[key]` on every tick | `nmp-ffi/src/lib.rs:1109` |
| `register_event_observer(arc)` | event-driven views | `on_kernel_event` fires per `Inserted\|Replaced` on actor thread | `event_observer.rs:189` |
| **ActionModule** (trait) | write seam shape | `NAMESPACE`, `type Action`, `start()`, `execute()` | `action.rs:56` |
| **CapabilityModule** (trait) | native bridge shape | request → native → result envelope (D7) | `capability.rs:11` |

Module composition: each module crate exports `pub fn register(app: &mut NmpApp) -> Store`; codegen wires them in `FfiApp::new`.

> **Removed:** `DomainModule`, `ViewModule`, `IdentityModule`, `ModuleRegistry` — never shipped. See [05a](05a-substrate-traits.md) §Removed v2 traits.

## Card 3 — v1 capability catalog (`docs/product-spec/api-surface.md:192-229` §6.5)

| Capability | Native does | Status |
|---|---|---|
| `KeyringCapability` | `store`/`load`/`delete`/`list` blobs | spec §6.5 |
| `PushCapability` | `register`/`unregister` | spec §6.5 |
| `ExternalSignerCapability` | `sign(SignRequest)` / `cancel` | spec §6.5 |
| `NetworkMonitorCapability` | `start`/`stop` | spec §6.5 |
| `BlobPickerCapability` | `pick(PickRequest)` | spec §6.5 |

These are the v1 catalog defined in the spec; the shipped substrate trait is
`CapabilityModule` (`substrate/capability.rs:11`). Every capability is
idempotent (`start` after `start` = no-op) and reports only — it never
decides retry, recovery, or routing (D7). Protocol-specific or app-specific
capabilities compose the same way.

## Card 4 — Doctrine D0–D10 (`docs/product-spec/doctrine.md:1-98`)

| # | Kind | One-liner |
|---|---|---|
| **D0** | policy | No app nouns in `nmp-core` — kernel + extension modules |
| **D1** | policy | Best-effort rendering: render now, refine in place; no spinner gates |
| **D2** | policy | Negentropy first, REQ second; every `(filter,relay)` has a watermark |
| **D3** | policy | Outbox routing automatic; manual relay selection is the opt-out |
| **D4** | policy | Single writer per fact; caches derive; no public cache-invalidation |
| **D5** | policy | Snapshots bounded by open views; the event store never crosses FFI |
| **D6** | substrate | Errors never cross FFI as exceptions — `toast: Option<String>` |
| **D7** | substrate | Capabilities report; never decide policy |
| **D8** | substrate | Composite reverse index · ≤60 Hz/view · working-set bounded · 0 hot-path allocs after warmup |
| **D9** | substrate | Kernel owns time — injected `Clock`; relay `created_at` is untrusted for policy |
| **D10** | policy | Provenance — private events (kind:1059 gift-wrap) never escape to public relays |

Conflicts resolve in listed order (D0 wins over D10).

## Card 5 — Planner pipeline + merge lattice

**4-stage compiler** (`crates/nmp-core/src/planner/`,
`docs/design/subscription-compilation/compiler.md`):

```
LogicalInterest[] ─▶ 1. resolve (mailbox/NIP-65 routing)
                  ─▶ 2. fallback (indexer set when mailbox unknown)
                  ─▶ 3. merge   (greedy, via the 9-rule lattice)
                  ─▶ 4. plan-id (content-address → CompiledPlan)
```

**9 merge-lattice rules** (`crates/nmp-core/src/planner/lattice/rules.rs`;
evaluated in `lattice/mod.rs` order 6, 9, 1, 2, 3, 4, 5, 7, 8):

| Rule | Field | Merge behaviour |
|---|---|---|
| 1 | `kinds` | union; empty (wildcard) absorbs |
| 2 | `tags` | per-key value union (bounded by `DEFAULT_VALUE_LIMIT`) |
| 3 | `since` | take the **min** (widen lower bound) |
| 4 | `until` | take the **max** (widen upper bound) |
| 5 | `limit` | merge only if both `None` |
| 6 | `lifecycle` | merge only if equal (checked first, cheap prune) |
| 7 | `event_ids` | union |
| 8 | `addresses` | union (`NaddrCoord`) |
| 9 | `relay_pin` | merge only if equal; wildcard `None` does **not** absorb `Some(_)` — the third routing lane (NIP-29 h-tag coalesce) |

## Card 6 — SyncStrategy decision matrix (`crates/nmp-nip77/src/coverage_gate.rs:49`)

`decide_strategy(key, GateInputs { coverage, capabilities, watermark })`:

| `Coverage` | relay `supports_nip77` | → `SyncStrategy` |
|---|---|---|
| `CompleteAsOf(_)` | any | `SkipReq` (cache authoritative — D2 forbids REQ) |
| `PartialUpTo`/`Unknown` | `Some(true)` | `NegThenReq` |
| `PartialUpTo`/`Unknown` | `Some(false)` / `None` | `ReqSince(synced_up_to + 1)`, or `ReqSince(0)` if no watermark |
| (any of the above) | + persisted resume blob | wrapped in `Resume { next, state }` |

`SyncStrategy::issues_wire_traffic()` is `true` for everything except
`SkipReq`.

## Card 7 — SnapshotProjection (the `projections` map seam)

| Field | Value |
|---|---|
| **What** | A named `nmp.*` slice of app/module state delivered in `KernelSnapshot.projections[key]` |
| **Register** | `register_snapshot_projection(key, Fn() -> serde_json::Value)` seam (`crates/nmp-ffi/src/lib.rs:1109`; C-ABI `snapshot.rs:83`; header `NmpCore.h:255`) |
| **Delivery** | Appended to the reactive push frame every emit tick — no pull symbol, no polling |
| **Read** | `snapshot.projections[key]` in the host `apply()` (e.g. `projections?.followList`, `KernelBridge.swift:884`) |
| **Exemplar** | `nmp-nip29/src/register.rs:66`; Chirp `register.rs:371` (`nmp.follow_list`); `nmp-nip57` (`nmp.nip57.zaps`) |
| **Typed sibling** | `register_typed_snapshot_projection` → `snapshot.typedProjections` (ADR-0037), **not** `projections[key]` |
| **Status** | Structural permanent — `ffi-deprecation-calendar.md:61` ("keep, freeze-locked") |

**Distinct from `KernelEventObserver`-driven view updates** (Card 2 seam 3) —
those push typed view deltas via `ViewBatch`; this is a named JSON state slice
in the snapshot's `projections` map. See [15](15-codegen-and-ffi.md) /
[17](17-ios-shell.md).

## Anti-patterns

- **Linking outdated variant lists.** Card 1 is `app.rs:1-30` at master tip.
  If you cite a `KernelAction` variant that is not in that file, you are
  describing a stale or aspirational surface — verify before quoting.
- **Conflating the long-term catalog with what ships today.** Card 3's
  capability traits are the spec §6.5 v1 catalog (marked `spec §6.5`); the
  shipped runtime contract is the single `CapabilityModule` trait. Do not
  present the five named traits as if they exist as Rust types on master.
- **Aspirational entries without a status marker.** Per-app
  `AppAction`/`AppUpdate`/`ViewSpec` are generated, not hand-written kernel
  enums — the callout box says so. Never table a generated or planned
  variant as if it were a fixed kernel API.
- **Reading the lattice rule order off this card as execution order.** The
  rule *numbers* are stable identities; the *evaluation* order
  (`lattice/mod.rs`) is 6, 9, 1, 2, … for early cheap pruning. Quote both.

See also: [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md), [05 — Kernel substrate — traits + seams](05a-substrate-traits.md), [16 — Capabilities (D7)](16-capabilities.md).
