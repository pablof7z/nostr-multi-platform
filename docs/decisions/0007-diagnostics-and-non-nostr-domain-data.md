# ADR-0007 — Diagnostics and non-Nostr data over the app bridge

- **Status:** Accepted
- **Date:** 2026-05-17

## Context

Apps need observability into relay health, subscriptions, cache coverage,
capabilities, local media, wallet state, sync jobs, and other non-Nostr facts.
Those facts must not cross as raw socket callbacks, raw planner callbacks, or
fake Nostr events.

## Decision

Diagnostics and non-Nostr data use the same actor-owned bridge discipline as
ordinary app state:

- Rust owns protocol state, retry policy, subscription policy, diagnostics, and
  durable non-Nostr records.
- Native reports raw capability facts and renders pushed state.
- Host-visible state crosses through typed snapshot envelope fields,
  projections, action status, or one-shot side effects.

## Network Observability

Networking is represented at three levels:

1. relay status: connection, auth, capability probes, counters, and last error;
2. wire subscription status: concrete REQs on concrete relays;
3. logical interest status: app/kernel interests and their coverage/degraded
   state.

These records are coalesced for UI consumption. They are not emitted per socket
frame or per byte counter change.

## Non-Nostr Data

Non-Nostr data enters one of these actor-owned lanes:

- durable domain records owned by a Rust module;
- action ledger rows for side effects and user intents;
- capability reports that Rust interprets;
- ephemeral side effects such as toasts, pairing URIs, or diagnostic file paths.

The event store remains Nostr-event storage. Local facts that are not Nostr
events live in their owning Rust module or store.

## Consequences

- Apps can render diagnostics without becoming protocol owners.
- Capability bridges stay policy-free.
- Debug surfaces inspect Rust-owned state through the same pushed bridge as
  product UI.
