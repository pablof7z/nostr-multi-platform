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

## Card 2 — The 5 trait families (`crates/nmp-core/src/substrate/`)

| Family | Owns | One-liner | File |
|---|---|---|---|
| **DomainModule** | durable records | schema version, migrations, `ingest_kinds()` | `domain.rs:1` |
| **ViewModule** | projections | `Spec`→`Payload`/`Delta`; declares reverse-index deps | `view.rs:37` |
| **ActionModule** | side effects | `start`→`ActionPlan`, `reduce`→`ActionTransition` | `action.rs:10` |
| **CapabilityModule** | native bridges | request → native → result envelope (D7) | `capability.rs:3` |
| **IdentityModule** | signing scopes | `scope_kind`, `create`, async `sign`, `destroy` | `identity.rs:8` |

Composed via `ModuleRegistry` (`substrate/mod.rs:38`).

## Card 3 — v1 capability catalog (`docs/product-spec/api-surface.md:192-229` §6.5)

| Capability | Native does | Status |
|---|---|---|
| `KeyringCapability` | `store`/`load`/`delete`/`list` blobs | spec §6.5 |
| `PushCapability` | `register`/`unregister` | spec §6.5 |
| `ExternalSignerCapability` | `sign(SignRequest)` / `cancel` | spec §6.5 |
| `NetworkMonitorCapability` | `start`/`stop` | spec §6.5 |
| `BlobPickerCapability` | `pick(PickRequest)` | spec §6.5 |

These are the v1 catalog defined in the spec; the shipped substrate trait is
`CapabilityModule` (`substrate/capability.rs:3`). Every capability is
idempotent (`start` after `start` = no-op) and reports only — it never
decides retry, recovery, or routing (D7). Protocol-specific or app-specific
capabilities compose the same way.

## Card 4 — Doctrine D0–D8 (`docs/product-spec/doctrine.md:1-98`)

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

Conflicts resolve in listed order (D0 wins over D8).

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

See also: [03 — Doctrine D0–D8 end-to-end](03-doctrine-d0-d8.md), [05 — Kernel substrate — the 5 trait families](05-substrate-traits.md), [16 — Capabilities (D7)](16-capabilities.md).
