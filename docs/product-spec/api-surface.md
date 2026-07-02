# Product Spec: API Surface

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

## 6. The Framework API Surface

The production surface is one opaque app handle, one push update callback, typed
capability sockets, typed projection registration, and one write doorway. Relay
routing, cache invalidation, subscription lifecycle, signing orchestration, and
store admission stay inside Rust.

Native app bindings target `crates/nmp-uniffi` over
`crates/nmp-native-runtime::NmpAppBuilder`. Browser bindings target
`nmp-browser-runtime` wasm-bindgen exports. Hot Rust-to-host updates are binary
`nmp.transport.UpdateFrame` frames. The frame carries a `SnapshotEnvelope` plus
typed projection payloads; production hosts decode typed frame data. Generated
Swift/Kotlin/TypeScript helpers decode those typed frames, but they do not define
a second runtime transport.

External interface policy: every public surface must earn its place before v1.
binding methods, wasm wire tags, FlatBuffers fields, projection keys, action
namespaces, generated bindings, CLI commands, and docs are cleaned in place when
the design improves. NMP does not retain pre-v1 compatibility shims, dead
parameters, app/example names, generic payload fallbacks, deprecated schema
slots, or old generated module paths solely to protect old callers.

### 6.1 App Handle And Lifecycle

Native hosts construct one UniFFI `NmpApp`, configure update sinks,
capabilities, storage, and projection declarations before start, then call the
UniFFI lifecycle method. Swift and Kotlin hold normal generated object handles;
Rust owns actor lifetime and teardown. All app data updates arrive through the
UniFFI update sink as byte frames; the callee copies bytes before the callback
returns.

Start/configure behavior is governed by visible-limit and emit-Hz clamps plus
Rust-owned policy. New public surfaces must not copy legacy transport parameters
or expose runtime policy as host-owned knobs.

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

Production write transport is ADR-0071 `DispatchEnvelope` bytes through the
UniFFI dispatch method or the equivalent wasm/browser runtime channel.
Host-facing builders expose typed intent methods; those builders encode the
per-module payload, stamp the namespace, and send the finished envelope through
the single doorway.

Write APIs separate three concerns:

- **construction:** protocol/app helpers build unsigned drafts or typed action
  payloads such as reply, reaction, article, group message, or DM;
- **signing:** the actor selects or receives the signer capability and records
  pending/rejected/signed state;
- **publishing:** the actor applies final protocol envelope mutation before
  signing, chooses route policy, publishes, ingests locally, and reports status.

Protocol helpers may override default outbox planning only through typed route
provenance, such as verified private inbox, group host pin, user-confirmed
override, imported event, or diagnostic route. Anonymous relay lists are not
durable product publish state.

Action modules are registered in Rust with `NmpApp::register_action`. A module
owns its action payload, validation, execution, capability requests, and any
state it projects. `nmp-core` does not grow app-specific verbs to satisfy one
consumer.

### 6.4 Reads, Sessions, And Projections

Production product reads are typed sessions, or generated per-feature helpers
over typed session descriptors. A session owns acquisition demand, bounded
catch-up, admission, source reconciliation, typed output, status, and teardown.
Raw acquisition machinery is substrate/protocol internals, not the app-facing
read model for product screens and not a public native-app door.

Current app-visible read helpers include feed/session and ref-resolution
surfaces, but their durable contract is ADR-0070's typed-session lifecycle.
`NmpApp` reference-resolution methods remain the refcounted typed hydration
surface for profile and event refs.

Feed-shaped product reads follow ADR-0076. The normal app-facing API is a
helper over typed sessions, for example
`app.feeds().open_spec(feed_key, feed_spec)` returning a `FeedHandle`. The
helper compiles through the standard NMP feed compiler and hides raw interests,
observer registration, source-effect hooks, projection registrars, pull
controllers, and teardown recipes from app/native callers. The current Rust
runtime facade also opens typed `FeedParams` directly; the `feed_key` /
`feed_spec` builder shape is ergonomic sugar over that descriptor, not a second
model. Lower-level crate-internal compiler seams are internal/test/composition
surfaces, not the taught product API.

Generated host-language helpers are allowed only as a convenience over that
same descriptor. `nmp gen feed-helpers` emits Swift/Kotlin/TypeScript helpers
for four source families — active-user-follows, active-user-hosted-groups,
list-members, and relay-set — each with typed `RootIndexed`/`Flat` shape
selection, constructing canonical `FeedParams` JSON and calling the platform
feed-session door (`openFeedJson` on native bindings, `feed_open_json` in
`runtime-web`). They do not choose a compiler, own feed reactivity, or replace
the handle-owned session lifecycle. There is no public raw observed-feed-source
doorway; app-owned row projection must land as a named typed read/session
contract or an app/protocol-owned recipe, not as a retained raw
event-sink escape hatch.

Feed descriptors declare app-owned keys, primary content kinds only, source
expressions, admission/order policy, bounded window policy, and an item
projection/schema contract. Protocol wrapper and maintenance kinds are derived
below the app boundary. Feed rows may expose stable refs, but profile hydration,
event embeds, reply counts, media, target hydration, and thread hydration belong
to the component/read model that renders those refs.

Current permanent concepts:

- typed session constructors for product reads;
- opaque session handles for teardown;
- generated helpers over the same session descriptors;
- typed projection/ref-resolution outputs.

Projection delivery is typed output transport. Projection keys, typed payloads,
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

`nmp init` scaffolds a thin Rust app shell with an explicit composition root for
selected substrate, protocol, app, signing/publish, and capability features.
`nmp gen typed-decoders`, projection-cache, keyed-ref-cache, and feed/action
helper emitters produce focused host helpers from the live UniFFI/schema
surface.

### 6.7 API Doctrine

- One write doorway per runtime.
- Construction, signing, and publishing are separate Rust-owned stages.
- Typed projections only; no schema-less payload fallback.
- Rust owns protocol and product policy; native renders and executes
  capabilities.
- App-specific behavior belongs in app Rust crates or reusable protocol crates,
  not in `nmp-core`.
- Pre-v1 compatibility shims do not define the current API.
