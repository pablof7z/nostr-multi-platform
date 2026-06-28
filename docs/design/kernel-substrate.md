# Design: Kernel Substrate

This file describes the current extension substrate.

## 1. Composition

Composition is app-owned Rust code using reusable installers:

1. Create an `NmpAppBuilder`.
2. Declare storage, output contracts, relays, and capabilities.
3. Install explicit substrate/protocol/app features.
4. Register app/protocol-specific actions, sessions, and typed outputs.
5. Start the app.

`nmp-core` provides the substrate. `nmp-defaults` provides reusable installers,
not hidden production app policy. App crates own their Rust product logic.

## 2. ActionModule

`ActionModule` is the write seam. It owns a namespace, typed action payload
decoding, validation, and execution into actor commands.

Hosts dispatch bytes or JSON envelopes to a namespace; the runtime registry
routes the payload to the registered module. The module's `execute` body is the
only place that translates an accepted user intent into kernel commands.

Use this seam for publish flows, relay-list edits, wallet operations, group
chat actions, signer actions, and app-specific commands.

## 3. Observed Projections

Observed projections declare the event shape, owner, scope, and replay bounds
for a Rust-owned read model before receiving events. Opening an observed
projection replays matching cached events to a muted sink, then activates future
delivery for the declared `InterestShape`.

Observed projections do not mutate native UI state directly. They update
Rust-owned state or cause projection emission through the normal snapshot/update
path.

## 4. Snapshot And Typed Projections

`register_typed_snapshot_projection` exposes named state slices in kernel snapshots.
Typed projection registration adds generated sidecars for hot paths that should
not rely on generic JSON walking.

Use projections for state the host renders. A projection is an output surface,
not a second source of truth; the owning Rust module still owns the facts and
policy.

## 5. Capabilities

Capabilities report native facts back to Rust. Native code can open files,
launch signers, inspect network state, or store secrets, but Rust decides what
the result means.

Capability requests and results are typed data crossing the Rust/native
boundary. Do not hide policy in native callbacks.

## 6. App-Owned Domain State

NMP does not provide a current generic domain-row registry. If an app needs
durable non-Nostr domain data, that app owns the Rust store and exposes the
renderable result through projections or actions. Shared Nostr mechanisms
belong in reusable NMP crates only when they are useful outside the app that
first needed them.

## 7. Diagnostics

Substrate behavior must remain observable through doctrine tests, typed
diagnostics, action results, relay/publish status, and projection output. A
module that cannot be tested through these seams is probably hiding state in
the wrong layer.
