# 0001 — FFI update-channel envelope (T103)

Status: **Superseded** · Scope: historical `nmp-core` actor emit boundary + `nmp-codegen` projection

> **Update (2026-05-26):** the runtime update transport is moving to one
> canonical FlatBuffers schema for Rust-to-frontend `FullState`, `ViewBatch`,
> and side-effect frames. This document now records the historical JSON
> envelope only. Do not add a production JSON fallback beside FlatBuffers.
> JSON remains valid for Nostr relay protocol frames, diagnostics/golden files,
> and temporary migration/test tooling where explicitly documented. UniFFI
> remains the binding/lifecycle/capability surface, not the hot payload format.

> **Update (2026-05-22):** the `t=update` arm (the discrete `WireDelta` /
> `DeltaEnvelope` channel) was **deleted** as shipped-but-inert. Every host
> bridge (iOS `KernelBridge.swift`, Android `KernelModel.kt`, desktop
> `bridge.rs`) explicitly dropped every `t=update` frame with a debug log;
> there were zero consumers. At the time, the remaining historical JSON
> contract was `{"t":"snapshot"}` and `{"t":"panic"}` (the D7 actor-death
> signal added later); everything
> below referring to the `Update` arm is historical context only.

## Problem

At the time of this ADR, the actor pushed updates to every host (Pulse, future
Android/desktop shells, `nmp-codegen`-projected enums) over a **single**
channel: `update_tx:
Sender<String>`. That channel carries **two structurally distinct JSON
shapes**:

1. **Discrete update** — `serde_json::to_string(&app::KernelUpdate)`, e.g.
   `{"ViewOpened":{"namespace":"profile","key":"pk"}}` /
   `{"UriRejected":{"uri":"…","reason":"…"}}`. Emitted from the
   `ActorCommand::Kernel` arm of `dispatch_command`.
2. **Periodic snapshot** — `Kernel::make_update()`, the large
   `{"rev":…,"items":[…],"metrics":{…},"open_views":…}` object every host
   renders. Emitted via `emit_now` (and every command/relay handler).

There is no discriminator. Every consumer had to **guess** which shape arrived
by sniffing keys (`"rev"` present? assume snapshot). That is undocumented,
fragile, and unsafe across the FFI seam — and impossible to model as one type
in a `nmp-codegen`-projected host enum.

## Decision

Wrap **every** frame on the channel in one **tagged outer envelope**:

```json
{"t":"update","v":<KernelUpdate>}
{"t":"snapshot","v":<snapshot>}
```

Serde contract: `#[serde(tag = "t", content = "v", rename_all = "snake_case")]`
over a two-variant enum `UpdateEnvelope { Update, Snapshot }`. The `t` values
are **exactly** `"update"` and `"snapshot"` (lowercase snake_case — pinned by
test, never the Rust `Update`/`Snapshot` variant casing).

Every consumer now decodes **one** discriminated type. The tag *is* the
discriminant (D6) — no key sniffing, no exceptions.

### Why a tagged outer envelope (vs. alternatives)

- **Merge `KernelUpdate` into the snapshot struct** — rejected: couples the
  discrete reducer result to the 30-field render snapshot; every discrete
  update would pay the full snapshot serialization cost (D8 violation).
- **Internally-tagged on each shape** — rejected: the snapshot is a foreign,
  already-serialized blob; we cannot inject a tag field without re-parsing it
  (D8 violation) and the discrete enum is externally tagged already.
- **Adjacently-tagged outer envelope (chosen)** — one cheap outer object, the
  snapshot is re-attached *by reference* (`serde_json::value::RawValue`) with
  no re-parse, and the result is a single `serde`-round-trippable type for
  every host and the codegen projection.

## Implementation

- `crates/nmp-core/src/update_envelope.rs` (new):
  - `WireEnvelope<'a>` — borrowing **emit-side** type
    (`Update(&KernelUpdate)` / `Snapshot(&RawValue)`); serialize-only.
  - `UpdateEnvelope` — owning **consumer-side** type
    (`Update(KernelUpdate)` / `Snapshot(serde_json::Value)`); the single type
    every host decodes into. The snapshot interior stays **opaque**
    (`serde_json::Value`) — this contract models the *discriminator*, not the
    snapshot's ~30 internal fields (which remain a crate-internal struct).
  - `wrap_update(&KernelUpdate) -> Option<String>` and
    `wrap_snapshot(String) -> Option<String>` helpers.
- `crates/nmp-core/src/actor/tick.rs`: `emit_now` wraps the snapshot via
  `wrap_snapshot`; new `emit_kernel_update` wraps the discrete update via
  `wrap_update`. Both wrap sites live in this one file.
- `crates/nmp-core/src/actor/mod.rs`: the `ActorCommand::Kernel` arm body now
  calls `emit_kernel_update(&update, update_tx)` (2-line body-of-arm change;
  no structural edit — `actor/mod.rs`/`ffi/mod.rs` structure is owned by the
  concurrent `softcap-split` session).
- `serde_json` gains the `raw_value` feature in `nmp-core/Cargo.toml`.
- `crates/nmp-codegen/src/generate.rs`: new `envelope_rs()` emits a generated
  host `UpdateEnvelope` with the identical serde contract; wired into
  `lib.rs` (`pub mod envelope; pub use envelope::UpdateEnvelope;`) and the
  generated `Cargo.toml` gains `serde`/`serde_json` deps.

### Codegen scope: wrap `KernelUpdate`, not `AppUpdate`

The generated `UpdateEnvelope::Update` wraps `nmp_core::KernelUpdate`
**directly**, not the projected `AppUpdate`. Rationale: only `Kernel(_)`
discrete updates ever flow on the streaming `update_tx` channel —
module-projected `AppUpdate` variants return through `FfiApp::dispatch`, not
the channel. Forcing serde onto generated `AppUpdate` would cascade serde
requirements onto every module's `Update` type (out of T103 scope, likely to
break compile). Carrying module-projected updates on the channel later is
**purely additive**: a new snake_case variant on the same `t` discriminator —
the envelope is not locked.

## Invariants

- **D6 (FFI clean):** the tag is the sole discriminant; serialization failure
  drops the frame (`None`) rather than unwinding across the seam.
- **D8 (allocation):** honest accounting — per snapshot frame: 1 alloc for
  `make_update` (unchanged) **+1** for the outer wrap (`to_string` copies the
  snapshot bytes + chrome once; `RawValue::from_string` validates and *takes
  ownership* of the box — no re-parse, no payload clone). Per discrete update:
  **0 extra** allocations vs. the pre-T103 `to_string(&update)`. O(n) in
  payload bytes; the wrap is cheap.

## Tests

- `update_envelope.rs`: tag-string casing pin; round-trip of **both** shapes
  through `UpdateEnvelope`; a consumer-side decode test proving a single
  decoder disambiguates the two shapes on one channel by tag alone;
  hand-written wire bytes decode (pins the format against accidental rename).
- `actor/tick.rs`: live-actor end-to-end — spawn `run_actor`, send `Start` +
  `Kernel(OpenView)`, drain the channel, assert every frame decodes as
  `UpdateEnvelope` and both variants appear.
- `nmp-codegen/tests/determinism.rs`: asserts the generated `envelope.rs`
  carries the canonical `t`/`v` snake_case tagging and both arms, and is wired
  into the generated crate (pins the wire contract from the codegen side).

Verified: `cargo test -p nmp-core -p nmp-codegen` and
`cargo clippy -p nmp-core -p nmp-codegen --all-targets -- -D warnings`.

---

## FFI-surface delta

This section is superseded by the FlatBuffers transport migration. The current
runtime callback ABI is `extern "C" fn(*mut c_void, *const u8, usize)`, and
`docs/ffi-surface.md` is the current source of truth for that surface.
