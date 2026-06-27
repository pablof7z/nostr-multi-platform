# 15 — Codegen: bindings + FFI surface

**Status:** raw C/JNI lifecycle/action FFI + FlatBuffers update transport SHIPS ·
UniFFI M14-0 (Android app-loop lane) SHIPS (issue #2129) · remaining M14 lanes PLANNED ·
`nmp init` thin-shell scaffold SHIPS · full multi-platform starter M16 PLANNED · Audience: both

A NMP app is a *composition*: one kernel + N protocol modules + 1 app core. The
canonical composition is delivered as a **library call**, not as generated wiring
in your source tree (ADR-0046 — see [19a](19a-walkthrough-microblog.md) and
[19b](19b-walkthrough-microblog.md) for how a new app uses it).

This section covers the generated *bindings* and the *FFI boundary*. The boundary
split is: the Android app-loop lane (lifecycle/action dispatch/update push) is
now served by UniFFI `AppHandle` (M14-0); raw C/JNI owns residual Android lanes
and all iOS lanes today; binary FlatBuffers owns the hot update stream; remaining
UniFFI M14 lanes are still planned; the full multi-platform starter remains M16.

## The `nmp.toml` manifest

The manifest parser in `crates/nmp-codegen/src/manifest.rs` survives only for
`nmp doctor` / `nmp upgrade` (dependency-policy commands). It is no longer used
to generate a per-app FFI crate. The parser recognises `[app]` and `[modules]`
sections; `[platforms]` keys are accepted but ignored.

```toml
# Example manifest — used today for `nmp doctor` / `nmp upgrade`
[app]
name         = "microblog"
display_name = "Microblog"

[modules]
kernel   = "nmp-core"
protocol = ["nmp-nip01"]
app      = ["microblog-core"]
```

## Composition: depend on `nmp-defaults`, call `register_defaults`

The canonical way to compose an app is a library call. In your app-core crate:

```rust
// microblog-core/src/lib.rs
use nmp_defaults::register_defaults;
use nmp_core::substrate::AppHost;

pub fn register(app: &mut impl AppHost) {
    // Inherit the canonical NMP composition (routing, outbox, DMs, zaps, WOT, ...).
    register_defaults(app);

    // Register app-specific modules / projections on top.
    // microblog_core::register_actions(app);
}
```

For a non-social app, make that choice inside the same app-core composition
root: call `nmp_defaults::register_substrate(app, gate)` instead of
`register_defaults(app)` when the app only needs the routable substrate. For
fine-grained feature toggles or policy overrides, use
`nmp_defaults::register_defaults_with(app, NmpDefaults { social: false,
..NmpDefaults::default() })`. See
`crates/nmp-defaults/src/lib.rs` and `crates/nmp-defaults/src/tiers.rs` for the
full API.

## What still gets generated

`nmp-codegen` retains the emitters that gate live CI:

- **`gen swift`** — Swift bindings for the C-ABI surface (`nmp_app_*`).
- **`gen typed-decoders`** — native decoders for the typed FlatBuffers projection
  sidecars carried in `SnapshotFrame.typed_projections`.

These are *bindings* (projections of a typed surface), not *composition wiring*.
Deleting the old `gen modules` scaffolder did not touch them.

## Current vs future FFI — read this box carefully

```
┌─ TODAY (SHIPS) ─────────────────────────────────────────────────────┐
│ Raw C/JNI lifecycle/action/capability ABI in crates/nmp-ffi. It      │
│ currently exports `nmp_app_*` (`new`, `start`, byte action dispatch, │
│ capability callbacks, projection/observer registration, etc.).         │
│ Target split (#2205/ADR-0068): nmp-ffi is only the C ABI shell over  │
│ nmp-native-runtime, which owns NmpApp, runtime slots, and builder.    │
│ The update callback carries one binary `nmp.transport.UpdateFrame`   │
│ with file identifier `NMPU`: Snapshot or Panic. There is no JSON     │
│ runtime snapshot fallback and no pull/drain update symbol.           │
│ There is NO generated per-app FFI crate; the app core calls          │
│ `nmp_defaults::register_defaults` and the raw C-ABI surface is shared. │
│ apps/chirp/ios consumes NmpCore.h backed by nmp-ffi plus Chirp wrappers.    │
├─ FlatBuffers runtime transport (SHIPS) ─────────────────────────────┤
│ One canonical transport frame carries typed SnapshotEnvelope fields  │
│ and typed projection sidecars from Rust to frontend shells. JSON is    │
│ allowed for Nostr relay frames, capability envelopes, diagnostics,     │
│ goldens, or tests. It is not a second production update transport.   │
├─ M14-0 — UniFFI Android app-loop lane (SHIPS — issue #2129) ────────┤
│ `AppHandle` UniFFI object (proc-macro, uniffi 0.29.5): `new()`,      │
│ `start()`, `stop()`, `close()`, `dispatch_action_bytes()`,           │
│ `dispatch_action_json()`, `dispatch_intent_json()`,                  │
│ `set_update_sink()`, `clear_update_sink()`. `UpdateSink` callback    │
│ interface delivers NMPU FlatBuffers frames (D8 push, no polling).    │
│ `DispatchAck` record: `correlation_id?`, `error?` (D6 — no throws). │
│ Generated Kotlin checked in at org/nmp/android/uniffi/; gated by    │
│ ci/check-uniffi-kotlin-drift.sh. FlatBuffers NOT transcoded.         │
├─ M14 remaining lanes — UniFFI (PLANNED) ─────────────────────────────┤
│ nmp-codegen extended to emit `uniffi::setup_scaffolding!()` +        │
│ lifecycle/binding wrappers (see ADR-0010 §Codegen output). iOS stops    │
│ importing NmpCore.h; imports the generated Swift module. UniFFI owns   │
│ object lifetime, callbacks, and capability interfaces; it is not the   │
│ hot update payload format. Residual Android JNI lanes (signer,        │
│ capability, marmot, identity, feeds) are staged for future migration.  │
├─ `nmp` CLI (SHIPS, crates/nmp-cli/) ────────────────────────────────┤
│ `nmp init <app>` scaffolds a thin Rust shell: a `<name>-core` crate  │
│ that calls `register_defaults`, plus a headless `examples/shell.rs`   │
│ that drives it through the native runtime `NmpAppBuilder`. No `gen   │
│ modules` step and no                                                  │
│ generated `apps/` tree. Full multi-platform starter is a future       │
│ milestone.                                                            │
└─────────────────────────────────────────────────────────────────────┘
```

ADR-0010 §"Codegen output" shows `#[derive(Clone, uniffi::Enum)]` and a
`bindings/{swift,kotlin,typescript}/` tree. **That is the M14 target shape, not
master.** Live `nmp-codegen` emits maintained host and runtime artifacts
(`gen swift`, `gen typed-decoders`, `gen projection-cache`, and
`gen builtin-keys`). UniFFI remains planned, and JSON is not a runtime fallback
for the update stream.

## How to add a snapshot projection to your app

> **Disambiguation — read this first.** This guide uses the word *projection*
> in two senses. The `ObservedProjectionSink`-driven view system (a reactive event
> fan-out into an app-owned store, described in [05a](05a-substrate-traits.md) +
> [06](06-reactivity-contract.md)) is one sense. **This section** is the other:
> a **snapshot projection** — an app/module-owned slice of state emitted inside
> the kernel's pushed update frame. In production host shells, that slice should
> be a typed FlatBuffers sidecar in `SnapshotFrame.typed_projections`, registered
> via `register_typed_snapshot_projection`.

**What it is.** A snapshot projection is a named slice of app- or module-owned
state, keyed by a dotted `nmp.*` namespace (e.g. `nmp.feed.home`,
`nmp.nip29.group_events`, `nmp.follow_list`, scoped relation-count projections),
that rides the kernel's reactive snapshot
push frame ([06 — Reactivity contract](06-reactivity-contract.md)) into the host.
The kernel pushes a **whole frame every emit tick when state changed**; hosts
decode the binary `UpdateFrame`, apply its `SnapshotEnvelope` fields, then read
projection sidecars by key. **No polling, no pull symbol** — render state arrives
on the callback path as part of the same frame as every other field.

Do not model zap counts as a global snapshot projection. Zap counts are
visible-note relation data: the owning card or detail view claims a bounded
`nmp.nip01.visible_note_relations` interest for its `#e=<event_id>` target.

### Production seam — `register_typed_snapshot_projection`

Register host-rendered projection state as a typed sidecar with the native
runtime registration API. Current C-ABI registration support lives in
`crates/nmp-ffi/src/snapshot.rs` until the ADR-0068 split moves runtime
ownership into `nmp-native-runtime`. The closure returns
`Option<TypedProjectionData>`:

- `Some(Changed row)` contains the projection key, e.g. `nmp.feed.home`;
  `schema_id`, `schema_version`, and FlatBuffers `file_identifier`; and the
  projection payload bytes, owned by the app/protocol crate that owns the
  schema.
- `None` means "no changed row this tick." Under incremental apply the host
  retains the last successfully decoded value for that key.
- `Cleared` is emitted by removing a registered typed key; the host drops any
  cached value for that key.

`nmp-core` treats those bytes as opaque. The host chooses the decoder by key and
descriptor and reads the generated native model from the `typed_projections`
vector. This is the production path for Swift/Kotlin/TS render inputs because it
uses typed transport data. Unknown host-visible state must get a typed sidecar
rather than a native JSON walker.

Idle or empty projections must still encode an empty snapshot payload when the
key is registered. Do not use `None` or sidecar absence to mean "empty wallet",
"idle signer", "no feed rows", or "not paired"; those are domain states inside
the schema. If a `Changed` row cannot be decoded, the host keeps the prior
value, does not advance the per-key applied rev, and requests/resumes from a
fresh baseline instead of committing an empty substitute.

The OP feed wiring is the canonical high-volume exemplar:
`nmp-defaults` registers the `nmp.feed.home` typed sidecar, `nmp-nip01` owns the
feed schema and encoder, and iOS decodes it through `TypedHomeFeedDecoder`
before assigning the corresponding `KernelModel` slot.

> **D8 + D6 — the projector runs on the actor thread inside the snapshot tick.**
> It MUST be cheap and non-blocking — no I/O, no mutex waits (D8); a blocking
> closure stalls every subsequent snapshot and freezes the host's update stream.
> Each closure is panic-isolated (`catch_unwind` per closure, D6:
> `crates/nmp-core/src/kernel/snapshot_registry.rs:125`), so a panic in one
> projector never aborts the snapshot.
