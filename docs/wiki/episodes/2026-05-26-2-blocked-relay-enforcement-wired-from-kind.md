---
type: episode-card
date: 2026-05-26
session: 64f3e239-c4c1-4c32-82de-458516b28418
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/64f3e239-c4c1-4c32-82de-458516b28418.jsonl
salience: product
status: active
subjects:
  - blocked-relay-set
  - kind-10006
  - outbox-routing
supersedes: []
related_claims: []
source_lines:
  - 1223-1236
  - 1388-1401
  - 1767-1803
captured_at: 2026-06-18T05:42:58Z
---

# Episode: Blocked relay enforcement wired from kind:10006

## Prior State

`BlockedRelaySet` struct existed in `routing.rs` with `contains()` checks on every routing lane, but `build_routing_context()` in `mailboxes.rs` always instantiated it as empty (`BlockedRelaySet::new()`). Kind:10006 events were ingested through the wildcard path with no parser — they went through `verify_and_persist` but were never interpreted.

## Trigger

User directive: kind:10006 must be subscribed at login and enforced in outbox routing — 'we don't autoconnect to blocked relay list through outbox to prevent connecting to malicious relays.'

## Decision

Add `BlockedRelayLookup` trait in `nmp-core/substrate/` (mirroring the existing `DmInboxRelayLookup` pattern), `InMemoryBlockedRelayCache` + `Kind10006Parser` in `nmp-router` (wire-shape parsing stays out of nmp-core per D0 doctrine), wire all 4 `build_routing_context()` call sites to read from the lookup via new `snapshot_blocked_relays()` helper, add `blocked_relays_slot` FFI pre-start slot on `NmpApp`.

## Consequences

- Every outbox routing lane now subtracts blocked relays — connections to kind:10006-listed relays are prevented at the routing layer
- Kind:10006 events are parsed by a registered `IngestParser` (not a kernel-side match arm), following the D0 doctrine that nmp-core must not name NIP-specific nouns
- Apps can provide custom `BlockedRelayLookup` implementations via the FFI slot; default is `EmptyBlockedRelayLookup` (safe no-op)

## Open Tail

- Trait placement in `nmp-core/substrate/` mirrors the `DmInboxRelayLookup` precedent but the original plan said 'BlockedRelayLookup must NOT go in nmp-core' — D0 is clean but the tension is noted

## Evidence

- transcript lines 1223-1236
- transcript lines 1388-1401
- transcript lines 1767-1803

