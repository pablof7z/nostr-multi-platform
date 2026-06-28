# Subscription Compilation §6 — Router-Owned Mailbox Routing

> **Status:** durable design note. `nmp-router` owns mailbox routing.

`nmp-router` owns the NIP-65 mailbox cache and the generic relay-routing
algorithm used by subscription compilation. The action namespace
`nmp.nip65.publish_relay_list` remains stable for callers, but its
implementation is part of the router crate rather than a standalone
`nmp-nip65` module.

## Owned By `nmp-router`

- `Kind10002Parser` decodes kind:10002 relay-list events and writes the shared
  `InMemoryMailboxCache`.
- `InMemoryMailboxCache` implements `nmp_core::substrate::MailboxCache`.
- `GenericOutboxRouter` implements `nmp_core::substrate::OutboxRouter`.
- `PublishRelayListAction` implements the
  `nmp.nip65.publish_relay_list` action namespace.
- `Nip65OutboxResolver` implements the publish-side resolver used by the
  publish engine.
- `IndexerRepublishPolicy` selects indexer targets for replaceable-event
  forwarding through the generic raw-event-forwarding seam.

## Owned By `nmp-core`

`nmp-core` owns only the substrate traits and actor seams:

- `MailboxCache`
- `OutboxRouter`
- `IngestParser`
- `ActionModule`
- publish resolver traits

The compile-time dependency from `nmp-router` to `nmp-core` is dependency
inversion: the trait lives in the substrate owner, and the concrete router is
injected by app composition.

## Composition

The explicit app composition root constructs one `Arc<InMemoryMailboxCache>` and
shares it between:

- the `GenericOutboxRouter` / `MailboxCache` pair installed into the kernel;
- the `Kind10002Parser` registered with the ingest dispatcher;
- the publish-side `Nip65OutboxResolver`.

This gives kind:10002 one writer and keeps the kernel from importing the router
crate directly.

## Non-Goals

- No per-NIP routing-rule registry.
- No protocol-specific routing branches in `nmp-core`.
- No standalone `nmp-nip65` crate.
- No socket lifecycle in `nmp-router`; sockets belong to `nmp-network`.
