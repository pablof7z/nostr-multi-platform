# ADR-0044 — Typed snapshot envelope fields

- **Status:** Accepted / implemented
- **Date:** 2026-06-10
- **Relates to:** ADR-0032, ADR-0037, ADR-0038

## Context

NMP's update transport uses a FlatBuffers `SnapshotFrame`. Some state is
projection-owned and belongs in typed projection sidecars. Other state is
framework-owned envelope metadata: revision, liveness, metrics, relay status,
logical interests, wire subscriptions, logs, and error diagnostics.

Envelope metadata is part of the transport contract itself, so it should be
typed directly on `SnapshotFrame`, not hidden in app/protocol sidecars.

## Decision

`SnapshotFrame` carries framework-owned envelope fields as first-class typed
FlatBuffers fields.

The envelope owns:

- `rev`;
- transport schema version;
- kernel schema version;
- actor liveness and run state;
- metrics;
- aggregate and per-relay status;
- logical interest status;
- wire subscription status;
- logs;
- optional error and store-open diagnostics;
- typed projection sidecars.

Nested framework shapes such as metrics and relay status are declared in the
transport namespace because `nmp-core` owns those concepts. App/protocol
projection shapes still live in their owning crates and travel through ADR-0037
sidecars.

## Version Axes

The transport frame version and the kernel snapshot schema version are separate
fields. The transport version answers "can this decoder read the frame shape?"
The kernel schema version answers "does this shell understand the kernel state
contract?" Consumers must not conflate them.

## Optional Fields

Optional diagnostics are encoded as optional scalar/string fields. Healthy frames
leave them absent; there is no wrapper object for presence.

## Consumer Contract

Hosts read envelope metadata from generated FlatBuffers bindings and decode
projection sidecars through the descriptor for each projection key. Unknown
projection descriptors fail closed at that sidecar boundary.

## Consequences

- Framework envelope metadata has a stable typed transport contract.
- App/protocol projection schemas remain outside `nmp-core`.
- Host decoders do not need a generic untyped snapshot tree to read production
  state.
