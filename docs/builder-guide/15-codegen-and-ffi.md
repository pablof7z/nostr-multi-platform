# 15 — Codegen: bindings + FFI surface

**Status:** UniFFI native binding + FlatBuffers update transport are the public native target ·
wasm-bindgen is the browser binding ·
`nmp init` thin-shell scaffold SHIPS · full multi-platform starter M16 PLANNED · Audience: both

A NMP app is a *composition*: one kernel + N protocol modules + 1 app core. The
canonical composition is delivered as a **library call**, not as generated wiring
in your source tree (ADR-0046 — see [19a](19a-walkthrough-microblog.md) and
[19b](19b-walkthrough-microblog.md) for how a new app uses it).

This section covers the generated *bindings* and the *FFI boundary*. The public
native binding is UniFFI for lifecycle, callbacks, and capability/object
bindings. Binary FlatBuffers remains the hot action/update payload through that
binding; UniFFI and FlatBuffers are complementary, not alternatives.
Browser/wasm uses the separate `wasm-bindgen` runtime surface. Any remaining
raw C/JNI symbols are internal/transitional compatibility, not starter-app API.
The full multi-platform starter remains M16.

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

## Composition: install explicit features

The canonical way to compose an app is explicit Rust composition. An app-core
crate installs the substrate, the reusable Nostr protocol features it wants, its
own app features, and any capability contracts its shell must execute.
`nmp-defaults` remains a reusable installer library; a hidden
`register_defaults()` preset is tutorial/test/migration compatibility, not
production architecture.

```rust
pub fn register(app: &mut impl AppHost) {
    let nmp_defaults::NmpDefaults {
        coverage_gate,
        search_defaults,
        ..
    } = nmp_defaults::NmpDefaults::default();

    let _mailbox_cache = nmp_defaults::register_substrate(app, coverage_gate);
    nmp_defaults::register_nip50_protocol_defaults(app);
    let _social_handles =
        nmp_defaults::register_social_protocol_defaults(app, search_defaults);
    nmp_defaults::register_dm_protocol_defaults(app);
    nmp_defaults::register_longform_projection(app);
    install_app_features(app);
    declare_capability_contracts(app);
}
```

The invariant is stable: the production root must show what substrate, protocol
features, app features, and capability contracts are installed. `register()`
must not collapse back to `register_defaults()` or a substrate-only starter.

## What still gets generated

`nmp-codegen` retains the emitters that gate live CI:

- **UniFFI native bindings** — Swift/Kotlin bindings for lifecycle, callbacks,
  capability objects, and byte action/update doorways.
- **`gen typed-decoders`** — native decoders for the typed FlatBuffers projection
  sidecars carried in `SnapshotFrame.typed_projections`.
- **Typed action builders** — generated host builders for declared action
  contracts, including app-local contracts once #2408's app-private kind lane is
  implemented.

These are *bindings* (projections of a typed surface), not *composition wiring*.
Deleting the old `gen modules` scaffolder did not touch them.

## App-private kind contracts (#2408)

An app can own a made-up event kind without upstreaming it into NMP and without
hand-rolling every builder. The durable contract is app-local input to NMP
tooling: it lives next to the app Rust crate and FlatBuffers schema, and it
describes the typed action surface that codegen should project into native and
web builders.

The app-private contract must name:

- the action namespace written into `DispatchEnvelope.action_namespace`;
- the event kind number and whether dispatch publishes a Nostr event or starts
  app-local work only;
- the FlatBuffers schema path, root type, file identifier, schema id, and schema
  version;
- the generated builder method name and flat-table field list/order;
- the owning Rust crate/module/type names for the app's `ActionPayload` and
  `ActionModule`;
- the Swift, Kotlin, and TypeScript generated-builder output targets;
- the drift/check commands the app runs in CI.

The first supported input form is a checked-in static JSON file consumed by
`nmp-codegen`:

```bash
cargo run -p nmp-codegen -- gen action-builders \
  --registry apps/<app>/action-builders.json \
  --platform swift \
  --out apps/<app>/ios/Generated/ActionBuilders.generated.swift \
  --check
```

Apps should normally run the registry-wide gate instead of spelling each output
path in CI:

```bash
cargo run -p nmp-codegen -- gen action-builders \
  --registry apps/<app>/action-builders.json \
  --check
```

That app-CI gate validates the static registry JSON, checks every declared
FlatBuffers schema path, `root_type`, `file_identifier`, and
`schema_version:uint` root field, then diffs the Swift, Kotlin, and TypeScript
generated-builder outputs declared by the registry. When it fails, regenerate
the stale output with the same `--registry` plus the specific `--platform`.

The JSON `actions` rows carry the namespace, event kind, dispatch kind, schema
identity, Rust owner types, and a flat-table `builder.fields` list in
FlatBuffers declaration order. The parser feeds only those static rows into the
builder emitters; it does not load schemas at runtime, discover plugins, or add
the app-private namespace to NMP's default `ACTION_CONTRACT` /
`ACTION_BUILDERS` tables.

Rust app code remains authoritative for meaning. The app crate owns validation,
tag policy, event construction, publish intent, and `ActionModule::execute`.
Generated builders only encode typed action bytes for the same byte doorway:
UniFFI native dispatch for Swift/Kotlin and wasm-bindgen `dispatch_bytes` for
TypeScript. They do not install modules, choose relays, create a per-app FFI
crate, or generate a composition root.

This is distinct from reusable NMP protocols. A generic Nostr mechanism that an
unrelated second app can consume unchanged belongs in a Layer-4 NMP crate and
may be wired by `nmp-defaults`. A product-private kind stays with the app while
using NMP's builder, binding, and drift-check machinery.

NMP repo CI continues to own only checked-in framework-generated surfaces:
default action builders, projection caches, typed decoders, producer constants,
and FlatBuffers binding drift for schemas in this repository. NMP CI does not
discover private app registries or remote schemas; each app that consumes an
app-local registry owns its own registry-wide `--check` command and Rust
decode/action-module tests.

## Public bindings and transitional internals

```
┌─ PUBLIC NATIVE BINDING ──────────────────────────────────────────────┐
│ UniFFI exposes lifecycle, callbacks, capability objects, and byte     │
│ action/update doorways to Swift/Kotlin/desktop native hosts.          │
│ Native shells import generated UniFFI modules; they do not call raw   │
│ C/JNI symbols as starter-app API.                                     │
│ The update callback carries one binary `nmp.transport.UpdateFrame`   │
│ with file identifier `NMPU`: Snapshot or Panic. There is no JSON     │
│ runtime snapshot fallback and no pull/drain update symbol.           │
│ There is NO generated per-app FFI crate; the app core owns explicit  │
│ Rust composition.                                                    │
├─ PUBLIC BROWSER BINDING ─────────────────────────────────────────────┤
│ wasm-bindgen exposes the browser runtime surface. Browser hosts use  │
│ the same FlatBuffers action/update bytes and browser capability      │
│ adapters; they do not share the native UniFFI object model.          │
├─ FlatBuffers runtime transport (SHIPS) ──────────────────────────────┤
│ One canonical transport frame carries typed SnapshotEnvelope fields  │
│ and typed projection sidecars from Rust to frontend shells. JSON is  │
│ allowed for Nostr relay frames, capability envelopes, diagnostics,   │
│ goldens, or tests. It is not a second production update transport.   │
├─ TRANSITIONAL / INTERNAL COMPATIBILITY ──────────────────────────────┤
│ Historical raw C/JNI `nmp_app_*` symbols may remain for migration,   │
│ tests, or narrow runtime internals. Do not teach them as the new app │
│ path; any residual byte lane must stay behind the public binding and │
│ be justified by measurement, not exposed as a second API.            │
├─ `nmp` CLI (SHIPS, crates/nmp-cli/) ────────────────────────────────┤
│ `nmp init <app>` scaffolds a thin Rust shell: a `<name>-core` crate  │
│ with an explicit composition root, plus a headless `examples/shell.rs`│
│ that drives it through nmp-native-runtime's `NmpAppBuilder`. No      │
│ `gen modules` step                                                   │
│ and no                                                               │
│ generated `apps/` tree. Full multi-platform starter is a future       │
│ milestone.                                                            │
└─────────────────────────────────────────────────────────────────────┘
```

ADR-0010 §"Codegen output" shows `#[derive(Clone, uniffi::Enum)]` and a
`bindings/{swift,kotlin,typescript}/` tree. Read it as binding-generation shape,
not as permission to generate composition wiring. Live `nmp-codegen` emits
maintained host and runtime artifacts (`gen typed-decoders`,
`gen projection-cache`, and `gen builtin-keys`). JSON is not a runtime fallback
for the update stream.

## How typed output reaches the shell

Typed output is the transport shape, not the app-facing read lifecycle. A
production screen opens a typed read session or generated helper. The session
executor may register typed output internally, but app developers should not
assemble raw interest, observer, replay, and projection wiring by hand.

The shell receives a pushed binary `UpdateFrame`, applies the
`SnapshotEnvelope`, and reads typed sidecars by key. No polling or generic pull
snapshot getter is allowed. Projection keys, sidecars, manifests, and change
gates are runtime/output machinery governed by ADR-0070 and ADR-0055.

Do not model zap counts as a global snapshot projection. Zap counts are
visible-note relation data: the owning card or detail view claims a bounded
`nmp.nip01.visible_note_relations` interest for its `#e=<event_id>` target.

### Internal seam — typed output registration

Session and protocol executors register host-rendered state as typed sidecars
with the runtime registration API. Runtime ownership stays in Rust and is
projected through the public bindings as typed output. The closure returns
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

The OP feed wiring is an implementation exemplar, not the public app API:
the session owner registers typed output, the protocol crate owns the schema and
encoder, and the host decodes the sidecar into a render cache.

> **D8 + D6 — typed output producers run on the actor update path.**
> It MUST be cheap and non-blocking — no I/O, no mutex waits (D8); a blocking
> producer stalls every subsequent update and freezes the host's update stream.
> Each closure is panic-isolated (`catch_unwind` per closure, D6:
> `crates/nmp-core/src/kernel/snapshot_registry.rs:125`), so a panic in one
> projector never aborts the snapshot.
