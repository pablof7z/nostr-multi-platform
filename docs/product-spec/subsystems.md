# Product Spec: Subsystems

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

## Event Store

Every event enters through one actor-owned insert path. The store verifies
events before mutation, applies replaceable/delete/expiration rules, records
provenance, and exposes bounded query APIs.

Coverage is tracked by the K3 coverage ledger:

- `record_coverage(filter_hash, relay, covered_through)`
- `get_coverage(filter_hash, relay)`
- `coverage_max_for_filter_hash(filter_hash)`
- `coverage_rows_for_filter_hash(filter_hash)`

A cache miss is authoritative only when coverage proves the relevant
`(filter_hash, relay)` window is complete.

## Subscription Planner

The planner maps materialized `LogicalInterest`s to relay-scoped wire REQs.
ReducedSources and dependent interests compile to logical interests before the
planner sees them.

The planner owns:

- live-tail before historical backfill,
- coverage-aware gap fill,
- logical-to-wire deduplication,
- formal filter merge rules,
- CLOSE for orphaned wire subscriptions,
- reconnect replay and gap repair.

## Routing And Publish

NIP-65 routing is the default policy for reads and writes. Protocol/app crates
ask for a publish or interest; routing resolves relay sets below that layer.

Publish state is Rust-owned. Relay attempts, ACK/NACK facts, retries, and
terminal status are visible through publish/action projections and diagnostics.
Native shells do not choose relays or retry policy.

## Sessions And Signers

User-visible accounts, active account selection, signer slots, and signer
operations are Rust-owned. `nmp-signers` owns concrete signer implementations;
native capabilities only execute OS-specific storage or external-signer facts.

Switching the active account is a state transition. Active-account-scoped
interests and signer bindings re-resolve from Rust state.

## Actions

Actions are registered through `ActionModule`. A module validates input,
receives a correlation id from the registry, and enqueues actor commands through
`execute`.

Action outcomes surface as typed results, projections, publish state, or
diagnostics. Errors do not cross FFI as exceptions.

## Projections

Hosts render snapshots and typed projection sidecars. Protocol/defaults/app
crates register the projections they own. A projection is an output surface, not
a second source of truth.

Projection payloads carry protocol facts and bounded state. Presentation
formatting belongs to the platform shell.

Zap totals are visible-target relation counts. NMP must not expose an
app-lifetime or process-wide zap aggregate projection.

## Capabilities

Capabilities report native facts: keychain results, external signer returns,
network changes, file handles, media facts, and similar OS-owned data. Rust
decides policy after receiving the fact.

## Extension Rule

Shared NMP crates own reusable Nostr infrastructure. App-specific domain state
belongs in the app's Rust core. Native shells render and execute OS handles
only.
