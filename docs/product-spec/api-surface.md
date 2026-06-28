# Product Spec: API Surface

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

## 6. The Framework API Surface

The production surface is one opaque app handle, one push update callback, typed
capability sockets, typed projection registration, and one write doorway. Relay
routing, cache invalidation, subscription lifecycle, signing orchestration, and
store admission stay inside Rust.

The native ABI is the hand-maintained raw C/JNI surface documented in
[`docs/ffi-surface.md`](../ffi-surface.md). Hot Rust-to-host updates are binary
`nmp.transport.UpdateFrame` frames. The frame carries a `SnapshotEnvelope` plus
typed projection sidecars; production hosts decode typed frame data.
Generated Swift/Kotlin/TypeScript helpers decode those typed frames, but they do
not define a second runtime transport.

External interface policy: every public surface must earn its place before v1.
FFI symbols, JNI methods, wasm wire tags, FlatBuffers fields, projection keys,
action namespaces, generated bindings, CLI commands, and docs are cleaned in
place when the design improves. NMP does not retain pre-v1 compatibility shims,
dead parameters, app/example names, generic payload fallbacks, deprecated schema
slots, or old generated module paths solely to protect old callers.

### 6.1 App Handle And Lifecycle

Native hosts allocate one `NmpApp` opaque handle with `nmp_app_new`, configure
callbacks and capabilities before start, then call `nmp_app_start`. `nmp_app_free`
is the only handle release path. All app data updates arrive through
`nmp_app_set_update_callback` as byte frames; the callee copies bytes before the
callback returns.

`nmp_app_start` and `nmp_app_configure` retain a dead pre-v1
`events_per_second` argument only for existing ABI shape. New callers must not
copy that parameter into new surfaces; start/configure behavior is governed by
visible-limit and emit-Hz clamps plus Rust-owned policy.

### 6.2 Host State

Host state is a bounded platform shadow reconstructed from `UpdateFrame` data.
It is screen-shaped: router/session/busy/toast/debug state plus only the open
projection payloads the UI renders. It never contains the whole event store,
gossip cache, signer internals, relay sockets, or durable store implementation
details.

The platform shadow is reorganized by domain key where that is ergonomic:
profiles by pubkey, event embeds by event id/address, feed rows by projection
session, relay diagnostics by relay URL. The owning Rust projection remains the
source of truth; native maps are render caches.

### 6.3 Actions And Write Transport

Production write transport is ADR-0064 `DispatchEnvelope` bytes through
`nmp_app_dispatch_action_bytes` or the equivalent wasm/browser runtime channel.
Host-facing builders expose typed intent methods; those builders encode the
per-module payload, stamp the namespace, and send the finished envelope through
the single doorway.

Action modules are registered in Rust with `NmpApp::register_action`. A module
owns its action payload, validation, execution, capability requests, and any
state it projects. `nmp-core` does not grow app-specific verbs to satisfy one
consumer.

### 6.4 Reads, Sessions, And Projections

Production product reads are typed sessions, or generated per-feature helpers
over typed session descriptors. A session owns acquisition demand, bounded replay,
admission, source reconciliation, typed output, status, and teardown.

Raw `nmp_app_open_interest` / `nmp_app_close_interest` is low-level acquisition
machinery for substrate, protocol-internal, diagnostic, export, test, or
migration scopes. It is not the app-facing read model for product screens.

Current app-visible read helpers include feed/session and ref-resolution
surfaces, but their durable contract is ADR-0070's typed-session lifecycle.
`nmp_app_resolve_ref` / `nmp_app_release_ref` remain the refcounted typed
hydration surface for profile and event refs.

Projection delivery is typed output transport. Projection keys, typed sidecars,
manifests, and change gates are how Rust pushes bounded session/app state to the
host; they are not the lifecycle an app developer hand-assembles.

### 6.5 Capabilities

Native shells execute capabilities; Rust decides policy. Capabilities report raw
facts back into the actor, where reducers decide the next state. Current native
capability families include keychain/keyring, external signer, lifecycle,
network, file/blob selection, and protocol-specific sockets such as signer
broker and NIP-55.

Capability errors are data, not exceptions. Missing handlers, malformed
requests, user cancellation, and provider failures surface through state or
typed result envelopes and never require native retry policy.

### 6.6 CLI And Generated Bindings

`nmp init` scaffolds a thin Rust app shell that composes `nmp-defaults` and
app-owned modules. `nmp gen swift` and `nmp gen typed-decoders` emit host helpers
from the live ABI/schema.

### 6.7 API Doctrine

- One write doorway per runtime.
- Typed projections only; no schema-less payload fallback.
- Rust owns protocol and product policy; native renders and executes
  capabilities.
- App-specific behavior belongs in app Rust crates or reusable protocol crates,
  not in `nmp-core`.
- Pre-v1 compatibility shims do not define the current API.
