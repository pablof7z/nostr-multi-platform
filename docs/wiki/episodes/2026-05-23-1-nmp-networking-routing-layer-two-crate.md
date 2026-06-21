---
type: episode-card
date: 2026-05-23
session: 1670fcb8-f275-498c-975b-8bd912331ded
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1670fcb8-f275-498c-975b-8bd912331ded.jsonl
salience: architecture
status: active
subjects:
  - nmp-router
  - nmp-network
  - routing-rule-registry
  - explicit-targets-seam
  - crate-boundaries
supersedes: []
related_claims: []
source_lines:
  - 13803-13882
  - 13932-14257
  - 14259-14273
  - 14455-14477
captured_at: 2026-06-18T05:11:56Z
---

# Episode: NMP networking/routing layer: two-crate split with explicit-targets seam replaces monolithic relay-pool with rule registry

## Prior State

The planned `nmp-relay-pool` crate (Layer 2) conflated routing decisions with socket/pool lifecycle, contained a per-NIP `RoutingRule` registry for extensibility, and owned all relay caches including kind:10050 DM-inbox. NDK and applesauce both mix routing with pool in ways that produce footguns (NDK #175 god-set, applesauce opt-in correctness).

## Trigger

Four-agent design exercise (audit + two parallel design agents + Codex prior-art research across applesauce, NDK, rust-nostr, Bitcoin Core) revealed that rust-nostr's `RelayPool` is `pub(crate)` with zero routing knowledge, routing lives in `Gossip` one layer up, and Bitcoin Core's `CConnman`/`PeerManager`/`AddrMan` triplet validates the same three-concern decomposition. The `RoutingRule` registry reproduces NDK's 'every NIP stamps the pool with its own routing intent' failure mode under a different shape.

## Decision

Split `nmp-relay-pool` into two crates: `nmp-router` (Layer 2, generic OutboxRouter algorithm + NIP-65 MailboxCache only) and `nmp-network` (Layer 1, sockets + pool lifecycle + push-model PoolEvent channel + generational RelayHandle). Delete the `RoutingRule` registry entirely; replace with `RoutingContext::explicit_targets: Option<&[RelayUrl]>` — NIP crates that already know their relay set pass it directly and the generic algorithm is bypassed. Pool API exposes only constrained per-handle sends; no 'send to all' method exists (structural answer to NDK #175). kind:10050 DM-inbox cache stays in `nmp-nip17` (its only consumer), not in the router.

## Consequences

- NIP crates register nothing with the router — no NIP nouns leak into the routing layer
- NIP-17 passes DM relays via explicit_targets from its own DmRelayCache; NIP-29 passes host relay from group state; Marmot passes MLS group relay — all bypass the generic algorithm
- Pool is not a public API; only the kernel actor holds both router and pool handles
- NIP-42 AUTH handling: network handles wire frames; subscription pause/replay is the planner's AuthGate
- Per-relay filter strategy = authors partitioning in nmp-planner::project_per_relay, not a routing concern; per-relay since cursors are explicitly out of scope (novel, would need separate ADR)
- Migration steps 2 and 8 in the 12-step plan now target nmp-router and nmp-network respectively; nmp-nip65 is absorbed into nmp-router
- MailboxCache in nmp-router is NIP-65 (kind:10002) only; IngestParser for kind:10002 registers via EventIngestDispatcher seam
- nmp-signer-broker retargets to nmp-network's Pool primitive instead of direct relay access

## Open Tail

- Per-relay since cursors remain an unaddressed novel primitive — orthogonal to routing, would live in nmp-store, needs its own ADR if pursued
- nmp-nip11 (NIP-11 INFO doc fetching, capability probing) called out as future crate but out of scope for current migration

## Evidence

- transcript lines 13803-13882
- transcript lines 13932-14257
- transcript lines 14259-14273
- transcript lines 14455-14477

