# FFI and Native Surface

> Canonical state after the M14 clean-break (EPIC-NS-001 / #2340). Authority:
> `docs/ffi-surface.md`, `docs/wasm-surface.md`, `docs/builder-guide/15-codegen-and-ffi.md`,
> ADR-0030, ADR-0067, ADR-0068, ADR-0072. If this file disagrees with those, fix them and
> re-derive this — do not fork the spec.

## The Two Binding Families

| Target | Public ABI | Owner crate | Payload |
|--------|-----------|-------------|---------|
| iOS, Android, desktop native | UniFFI | `crates/nmp-uniffi` | FlatBuffers `Vec<u8>` frames |
| Browser (wasm) | wasm-bindgen | `crates/nmp-browser-runtime::wasm` | FlatBuffers `Uint8Array` frames |

These are separate binding families. Do not route browser guidance through UniFFI. Do not
use browser/wasm as a reason to retain legacy native C symbols.

## Native Stack (three layers)

```
┌─ nmp-uniffi ────────────────────────────────────────────────────────────┐
│ Public native generated surface. One uniffi::setup_scaffolding!().       │
│ Exposes: NmpApp lifecycle, UpdateSink callback, dispatch_action(Vec<u8>),│
│ identity/signer/relay/session/capability/publish controls, mirror pull.  │
├─ nmp-uniffi-support ─────────────────────────────────────────────────────┤
│ Shared Rust mechanics: panic containment, quiescence, dispatch, clamp.   │
│ NO setup_scaffolding!(). Shared only through Rust-side function calls.    │
├─ nmp-native-runtime ─────────────────────────────────────────────────────┤
│ Runtime adapter. Owns NmpApp handle, actor-thread lifecycle, native      │
│ session registries, NmpAppBuilder typestate, RunConfig.                  │
└──────────────────────────────────────────────────────────────────────────┘
```

Composition is owner-local: app roots call `nmp_substrate::install(...)` plus named
protocol/runtime installers from the crates that own those mechanisms. This composition has no
platform runtime handle, no C ABI, and no lifecycle. See
`composition-and-product-policy.md`.

## UniFFI Is the Sole Public Native ABI

`crates/nmp-uniffi` (one `uniffi::setup_scaffolding!()`) is the reusable framework native
surface. A new `pub extern "C"` in any `crates/nmp-*` framework crate is an **ABI
regression** and requires an ADR-0030 exception gate (measured hot-path failure + internal
wrapper behind a UniFFI API + named owner + thresholds + delete trigger).

## FlatBuffers Ride Through UniFFI, Not Alongside It

FlatBuffers are the wire payload; they are not UniFFI records.

- **Into Rust:** `NmpApp::dispatch_action(Vec<u8>)` — caller builds a `DispatchEnvelope`
  (file identifier `NMPD`) from generated typed builders.
- **Out of Rust:** `UpdateSink::on_update(Vec<u8>)` push callback — carries an `UpdateFrame`
  (`NMPU`) with `SnapshotEnvelope` fields and typed projection rows. Push only; no pull/drain
  accessor.

UniFFI owns object lifetimes, callback generation, and host-language type projection. It does
not own or transcode action/update schemas — those stay in FlatBuffers/codegen crates. The
boundary contract: a bounded, screen-shaped typed frame leaves; a typed action envelope
enters. The event store, signer internals, relay watermarks, and history never cross.

Performance gate:
`cargo run -p nmp-testing --bin ffi-transport-bench --release -- --standard --fail-on-gate`.

## App-Owned UniFFI Facades

When an app needs **app-specific native verbs** (not reusable framework verbs), it owns **one**
facade crate calling `uniffi::setup_scaffolding!()`.

### The facade crate contains
- `uniffi::setup_scaffolding!()` — once, in this crate only.
- A facade-local UniFFI object (e.g. `TwentyNinerApp`).
- Facade-local records and callback interfaces (`DispatchOutcome`, `UpdateSink`,
  `CapabilitySink`) in the facade's generated Swift/Kotlin namespace.
- Thin adapters translating facade-local types into `nmp-uniffi-support` helpers.

### The facade crate MUST NOT contain
- Copies of lifecycle, start, configure, clamp, update-sink registration, capability delivery,
  dispatch, panic containment, or quiescence mechanics. These live in `nmp-uniffi-support` /
  `nmp-native-runtime`; the facade adapts its local types into those helpers and copies zero
  policy.

### Namespace isolation (hard constraint)
UniFFI's `uniffi-bindgen --library` resolves every exported record and callback interface to
its owning facade namespace. You cannot export shared UniFFI records or callback interfaces
cross-crate from `nmp-uniffi-support` (it deliberately does not call `setup_scaffolding!()`).
Facade-local shim types must be defined in the app crate. **Rust compile alone does not prove
the generated native namespace.** Prove it:

```bash
uniffi-bindgen generate --library <app-cdylib> --language swift  --out-dir <swift-out>
uniffi-bindgen generate --library <app-cdylib> --language kotlin --out-dir <kotlin-out>
```

### Facade vs upstreaming
A generic Nostr mechanism a second app could reuse unchanged belongs in a Layer-4 NMP crate
and the reusable `nmp-uniffi` surface. A product-private verb stays in the app facade. Native
shells in both cases only render state and execute raw capabilities.

## `nmp-uniffi-support` Shares Mechanics, Not Types

Reuse — never copy into a facade — these helpers: `start_runtime`, `configure_runtime`,
`clamp_visible`, `clamp_emit_hz`, `set_update_sink`/`update_listener_from_sink`,
`set_capability_callback`/`capability_handler_from_sink`, `dispatch_capability_json`,
`dispatch_action`/`dispatch_action_vec`, `register_action_result_observer`,
`set_lifecycle_callback`.

## What Is Deleted (Do Not Resurrect)

| Deleted artifact | Replacement |
|------------------|-------------|
| `crates/nmp-ffi` crate | `crates/nmp-uniffi` + `nmp-native-runtime` |
| Framework C-ABI symbols (`nmp_app_lifecycle_*`, `nmp_app_dispatch_action`, …) | UniFFI `NmpApp` methods |
| `crates/nmp-wasm` | `crates/nmp-browser-runtime::wasm` |
| Marmot C-ABI public framework surface | Internal; post-v1 direction tracked in #2232 |

References to deleted symbols may appear only as deleted history or test evidence. New native
guidance must name the UniFFI method, generated binding, or Rust runtime seam.

## Remaining App-Owned `extern "C"` Glue

Raw `extern "C"` may remain in `apps/` as app-owned delivery glue (e.g.
`apps/nmp-gallery/crates/nmp-app-gallery/src/`). It is app-owned, must carry an owning issue,
and must not be promoted into reusable NMP framework API. It is not a second native binding
family. Any proposal to add a new raw lane after its UniFFI replacement exists requires an
ADR-0030 exception.

## Browser Stack

```
nmp-browser-runtime::wasm (wasm-bindgen Worker export)
  ├── NmpWasmRuntime: JSON control channel        (handle_json)
  ├──                 binary write channel         (handle_dispatch_bytes)  ← NMPD bytes
  └──                 binary snapshot callback      (set_snapshot_callback)  ← NMPU bytes
```

TypeScript renders snapshots and executes browser capabilities; Rust owns policy, routing,
replay, protocol behavior, and state transitions. Browser durable mode requires real
Worker/OPFS init before product start — silent in-memory fallback is not product success
(see `runtime-capability-shell-boundary.md`).

## Gates

| Change | Required gate |
|--------|--------------|
| Any change to `crates/nmp-uniffi/` interfaces | `bash ci/check-uniffi-bindings-drift.sh` |
| App facade `setup_scaffolding!()` | `uniffi-bindgen generate --library` for Swift + Kotlin |
| UniFFI byte transport performance | `ffi-transport-bench --standard --fail-on-gate` |
| Doctrine-sensitive NMP change | `cargo test -p nmp-testing --test doctrine_lint_smoke` |

## Hard Rules

- `pub extern "C"` in `crates/nmp-*` is a framework ABI regression: file an ADR-0030 exception
  or remove it.
- `uniffi::setup_scaffolding!()` appears exactly once per linked cdylib — in `nmp-uniffi` for
  framework-only apps, or in the app facade crate for apps with app-specific verbs.
- `nmp-uniffi-support` shares Rust mechanics; it never exports UniFFI types directly.
- Composition installers are Rust `AppHost` registrations; they are not native ABI or browser
  ABI.
- Capability bridges report raw results; Rust decides meaning (unchanged by the migration).
- The event store, signer internals, relay watermarks, and history never cross the boundary.
