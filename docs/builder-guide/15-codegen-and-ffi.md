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
Browser/wasm uses the separate `wasm-bindgen` runtime surface. App-owned raw
glue is delivery-specific and is not starter-app API.
The full multi-platform starter remains M16.

## The `nmp.toml` manifest

The manifest parser in `crates/nmp-codegen/src/manifest.rs` survives only for
`nmp upgrade` dependency-policy rewrites. It is no longer used to generate a
per-app FFI crate. The parser recognises `[app]` and `[modules]` sections;
`[platforms]` keys are accepted but ignored.

```toml
# Example manifest — used today for `nmp upgrade`
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
`nmp-substrate` provides only the shared substrate floor. Protocol and app
features are installed by their owner crates; hidden presets and replacement
defaults bundles are not production, tutorial, migration, or test architecture.

```rust
pub fn register(app: &mut impl AppHost) {
    let _substrate_handles =
        nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());
    nmp_nip50::register_search_scopes(app);
    nmp_nip50::register_input_scopes(app);
    nmp_nip02::register_follow_actions(app);
    nmp_replies::register_actions(app);
    nmp_nip17::register_actions(app);
    nmp_nip17::register_runtime(app);
    let _comment_runtime = nmp_nip22::register_runtime(app);
    nmp_nip23::register_longform_projection(app);
    install_app_features(app);
    declare_capability_contracts(app);
}
```

The invariant is stable: the production root must show what substrate, protocol
features, app features, and capability contracts are installed. `register()`
must not collapse back to a broad hidden preset or a substrate-only starter.

## What still gets generated

`nmp-codegen` retains the emitters that gate live CI:

- **UniFFI native bindings** — Swift/Kotlin bindings for lifecycle, callbacks,
  capability objects, and byte action/update doorways.
- **`gen typed-decoders`** — native decoders for the typed FlatBuffers projection
  rows carried in `SnapshotFrame.typed_projections`.
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
is wired explicitly by app/runtime composition roots. A product-private kind
stays with the app while using NMP's builder, binding, and drift-check
machinery.

NMP repo CI continues to own only checked-in framework-generated surfaces:
default action builders, projection caches, typed decoders, producer constants,
and FlatBuffers binding drift for schemas in this repository. NMP CI does not
discover private app registries or remote schemas; each app that consumes an
app-local registry owns its own registry-wide `--check` command and Rust
decode/action-module tests.

### Starter proof: `nmp-example-login-timeline`

`crates/nmp-example-login-timeline` is the public starter proof for one
app-private kind. Follow these files as the smallest complete shape:

- `action-builders.json` declares
  `app.login_timeline.publish_status`, event kind `30444`,
  `schema/publish_status.fbs`, generated builder method `publishStatus`, and
  the app-owned Rust payload/module names.
- `schema/publish_status.fbs` is the app-owned FlatBuffers payload schema.
- `generated/ActionBuilders.generated.swift`, `generated/ActionBuilders.kt`,
  and `generated/actionBuilders.generated.ts` are generated from that app-local
  registry. They build `DispatchEnvelope` bytes for the byte doorway; they do
  not call a raw publish escape or `nmp.publish`.
- `src/private_status.rs` owns `PublishStatusAction: ActionPayload`,
  `PublishStatusModule: ActionModule`, validation, tag policy, and event
  construction for kind `30444`.
- `src/lib.rs` registers `PublishStatusModule` in the app composition root with
  `ActionRegistrar::register_action`.
- `src/private_status_tests.rs` decodes generated-builder-shaped bytes through
  the app-owned `ActionPayload`, registers the app-owned `ActionModule`, and
  asserts execution publishes the declared app-private kind.

Regenerate and check the starter proof with:

```bash
cargo run -p nmp-codegen -- gen action-builders \
  --registry crates/nmp-example-login-timeline/action-builders.json \
  --check
cargo test -p nmp-example-login-timeline
```

That path is intentionally app-local. Do not add
`app.login_timeline.publish_status` to NMP's built-in `ACTION_CONTRACT` or
`ACTION_BUILDERS`; the built-in tables are only for reusable default NMP
actions.

The binding-surface counterpart to this kind lane is
[App-owned UniFFI facades (#2494)](#app-owned-uniffi-facades-2494): the same
first-class-extender principle one layer down, for app-specific *native verbs*
instead of app-specific *kinds*.

## App-owned UniFFI facades (#2494)

Reusable framework verbs reach Swift/Kotlin through the stock `nmp-uniffi`
generated namespace. When your app needs app-specific protocol verbs — verbs
that are not reusable framework verbs — you do not stand up a parallel binding
crate or hand-write a C bridge. You own **one** app-facade UniFFI crate layered
over `nmp-native-runtime` and `crates/nmp-uniffi-support`.

What you write in the facade crate:

- `uniffi::setup_scaffolding!()` once (your facade is the single cdylib the
  native shell links);
- a facade-local UniFFI object (e.g. `TwentyNinerApp`), plus facade-local
  records and callback interfaces, all in your own generated Swift/Kotlin
  namespace;
- thin adapters that translate your local UniFFI traits/records into the shared
  helpers below.

What you MUST reuse, never copy:

- lifecycle/start/configure clamp policy (`nmp_uniffi_support::start_runtime`,
  `configure_runtime`, `clamp_visible`, `clamp_emit_hz`);
- update-sink registration and panic-contained delivery
  (`set_update_sink` / `update_listener_from_sink`);
- capability callbacks (`set_capability_callback` /
  `capability_handler_from_sink`, `dispatch_capability_json`);
- action dispatch and its typed outcome (`dispatch_action` /
  `dispatch_action_vec` returning `nmp_uniffi_support::DispatchOutcome`);
- action-result observers and lifecycle observers
  (`register_action_result_observer`, `set_lifecycle_callback`);
- feed/projection session open/close/reopen
  (`open_feed_session`, `close_feed_session`, `reopen_feed_session`);
- active-account-change observation (`register_account_change_sink`,
  `unregister_account_change_sink`).

These mechanics — panic containment, quiescence, dispatch, clamp policy, session
teardown, account-change observation — live below the generated surface in
`nmp-uniffi-support` / `nmp-native-runtime`. Copy zero runtime bridge policy into
your facade.

Feed-session helpers are bridge mechanics below ADR-0076's app-facing helper
shape. Generated or app-owned facades should teach product code to open
feed-shaped typed sessions, for example `app.feeds().open_spec(feed_key, feed_spec)`,
while reusing `open_feed_session` / `close_feed_session` internally where JSON
descriptor bridging is still the local binding shape. Do not expose compiler
selection, observer registration, raw interest JSON, pull-controller wiring, or
teardown recipes through a product facade.

Use `nmp gen feed-helpers --platform swift|kotlin|ts --out <path>` when a host
binding wants checked generated helper code over that JSON bridge. The generated
helpers cover four source families — active-user-follows, active-user-hosted-groups,
list-members, and relay-set — each building canonical `FeedParams` JSON with
typed `RootIndexed`/`Flat` shape selection and calling the platform feed-session
door (`openFeedJson` on native bindings, `feed_open_json` in `runtime-web`);
they do not create a second runtime path. Rust app crates should prefer the
typed shape directly:

```rust
let handle = app.feeds().open_spec(
    FeedKey::app("app.example.home")?,
    feed::events()
        .primary_kinds([1])
        .from(source::active_user().follows())
        .shape(FeedShape::RootIndexed)
        .order(FeedOrder::NewestByFeedPosition)
        .window(FeedWindowPolicy::bounded(80))
        .project(FeedItemProjection::feed_rows()),
)?;
```

### Account-change and account-scoped sessions (#2516)

Two layers, pick the lighter one. Account-**reactive** feeds
(`FeedSourceExpr::ActiveUserFollows`) re-seed in place on an active-account change —
the native runtime's identity-change wiring rebuilds the live session, so your
facade does nothing. Account-**pinned** app-specific sessions (e.g. a NIP-29
joined-groups view bound to the active account) observe the change with
`register_account_change_sink` and rebuild with `reopen_feed_session` from one
of your facade methods.

Do this through the helpers, not a raw runtime pointer. Your facade owns its
`NmpApp` by value inside its `Arc<Facade>` UniFFI object and passes `&self.inner`
to each helper; the helpers borrow `&NmpApp` and forward callbacks through
`Arc`-held sinks. There is no sanctioned `*mut NmpApp` to capture — that pattern
belonged to the deleted raw native builder lane. Reaching for a raw runtime pointer
in a facade is a smell.

### Why facade-local records (the hard constraint)

You cannot export shared UniFFI records or callback interfaces cross-crate from
`nmp-uniffi-support`. UniFFI's namespace model resolves every exported record and
callback interface to its owning facade namespace; an earlier attempt that
exported them directly from `nmp-uniffi-support` compiled in Rust but failed at
`uniffi-bindgen --library` with `Unknown namespace for CallbackInterface(...)
(nmp_uniffi_support)` when generating bindings from both `nmp-uniffi` and the
app. That is why `nmp-uniffi-support` deliberately does **not** call
`setup_scaffolding!()`: it shares only the Rust-side mechanics. So you still
define tiny local shims (your own `DispatchOutcome`, `UpdateSink`,
`CapabilitySink` types) — but they delegate straight to the shared helpers and
carry no policy of their own.

### Boundary rule vs. upstreaming to NMP

App facades are for app-owned verbs and composition roots, not for bypassing NMP
ownership. A generic Nostr mechanism that an unrelated second app could reuse
unchanged belongs in a Layer-4 NMP protocol crate (and the reusable `nmp-uniffi`
surface), not in your app facade — exactly as a generic kind belongs upstream
rather than in an app-private action registry. Native shells still only render
state and execute raw capabilities.

### Worked example: 29er

The NIP-29 groups app 29er owns a `TwentyNinerApp` facade — an app-owned
composition root and generated native namespace, not a place where NIP-29
protocol semantics live. The facade exposes app-specific verbs such as
`createGroupPost` and `openGroupDiscovery` to its iOS/Android shells as
first-class native citizens, with facade-local `CapabilitySink`,
`UpdateSink`, and `DispatchOutcome` types in the `TwentyNinerApp` namespace.

Each facade verb composes reusable mechanics rather than reimplementing
them, and the composition stays on the right side of the kind-blind
boundary ([#2506](https://github.com/pablof7z/nostr-multi-platform/issues/2506),
[#2509](https://github.com/pablof7z/nostr-multi-platform/issues/2509)):

- group routing and session machinery — h-tag publish-into-group, relay
  discovery interests, joined-groups subscriptions — stays in `nmp-nip29`,
  which is kind-blind transport and carries no `react_in_group` /
  `repost_in_group` / per-kind helpers;
- a foreign action a user takes *inside* a group (reacting, reposting,
  replying, deleting) is built by the concept crate that owns that kind
  (`nmp-nip25` for reactions, `nmp-replies` for replies, and so on) and
  handed to `nmp-nip29`'s one generic publish-into-group entry point for
  routing. The facade never asks `nmp-nip29` for a kind-named verb, and it
  never constructs those foreign events itself.

The facade adapts those composed calls into local types and the
`nmp-uniffi-support` helpers; it copies none of NMP's lifecycle, dispatch,
or clamp policy, and it does not promote reusable, kind-blind group
mechanics into app-specific protocol verbs — only the composition is
app-owned.

### Validation

A Rust compile alone does **not** prove the generated native namespace — the
namespace failure above only surfaces at bindings generation. An app-facade proof
must generate bindings for both languages against the app cdylib and confirm the
facade's verbs appear:

```bash
uniffi-bindgen generate --library <app-cdylib> --language swift --out-dir <swift-out>
uniffi-bindgen generate --library <app-cdylib> --language kotlin --out-dir <kotlin-out>
```

For the reusable framework surface, drift is gated by
`bash ci/check-uniffi-bindings-drift.sh`. The durable rationale and binding-surface
rules live in [`docs/ffi-surface.md`](../ffi-surface.md) ("App-Owned UniFFI
Facades" / "Verification Pointers") and
[ADR-0030](../decisions/0030-uniffi-vs-c-abi.md); this guide is only the
app-author how-to.

## Public bindings and transitional internals

```
┌─ PUBLIC NATIVE BINDING ──────────────────────────────────────────────┐
│ UniFFI exposes lifecycle, callbacks, capability objects, and byte     │
│ action/update doorways to Swift/Kotlin/desktop native hosts.          │
│ Native shells import generated UniFFI modules; they do not call       │
│ deleted framework symbols as starter-app API.                         │
│ An app may also own a UniFFI facade for app-specific verbs (#2494).   │
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
│ and typed projection rows from Rust to frontend shells. JSON is      │
│ allowed for Nostr relay frames, capability envelopes, diagnostics,   │
│ goldens, or tests. It is not a second production update transport.   │
├─ APP-OWNED DELIVERY GLUE ────────────────────────────────────────────┤
│ App-specific raw glue may exist for local adapters such as Gallery.  │
│ It is not reusable framework API; any residual byte lane must stay   │
│ behind the public binding and be justified by measurement, not       │
│ exposed as a second API.                                             │
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
`SnapshotEnvelope`, and reads typed output rows by key. No polling or generic
pull snapshot getter is allowed. Projection keys, output manifests, and change
gates are runtime/output machinery governed by ADR-0070 and ADR-0055.

Do not model zap counts as a global snapshot projection or a shared relation
bucket. The owning card or detail view asks the zap concept owner for a bounded
target read; app-owned social bars compose concept-owned reads rather than
claiming a central relation namespace.

### Internal seam — typed output registration

Session and protocol executors register host-rendered state as typed output rows
with the runtime registration API. Runtime ownership stays in Rust and is
projected through the public bindings as typed output. The closure returns
`Option<TypedProjectionData>`:

- `Some(Changed row)` contains the app/protocol-owned projection key, e.g.
  `myapp.timeline.home`;
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
uses typed transport data. Unknown host-visible state must get a typed output row
rather than a native JSON walker.

Idle or empty projections must still encode an empty snapshot payload when the
key is registered. Do not use `None` or row absence to mean "empty wallet",
"idle signer", "no feed rows", or "not paired"; those are domain states inside
the schema. If a `Changed` row cannot be decoded, the host keeps the prior
value, does not advance the per-key applied rev, and requests/resumes from a
fresh baseline instead of committing an empty substitute.

The OP feed wiring is an implementation exemplar, not the public app API:
the session owner registers typed output, the protocol crate owns the schema and
encoder, and the host decodes the output row into a render cache.

> **D8 + D6 — typed output producers run on the actor update path.**
> It MUST be cheap and non-blocking — no I/O, no mutex waits (D8); a blocking
> producer stalls every subsequent update and freezes the host's update stream.
> Each closure is panic-isolated (`catch_unwind` per closure, D6:
> `crates/nmp-core/src/kernel/snapshot_registry.rs:125`), so a panic in one
> projector never aborts the snapshot.
