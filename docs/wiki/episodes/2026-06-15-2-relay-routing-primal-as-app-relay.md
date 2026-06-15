---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: active
subjects:
  - nmp-chirp-config
  - nmp-router-discovery
  - relay-roles
supersedes:
  - 2026-06-15-2-primal-net-relay-role-flipped-to
  - 2026-06-15-3-relay-nostr-band-purged-auth-walls
related_claims: []
source_lines:
  - 3191-3745
captured_at: 2026-06-15T11:23:07Z
---

# Episode: Relay routing: primal as app relay, discovery additive to app relays, dead relay purged

## Prior State

relay.primal.net was configured as both,indexer (app relay + dedicated indexer). The kind:10002 discovery probe targeted only indexer_relays. relay.nostr.band appeared in 13 files (tests, docs, seed configs) but AUTH-walls anonymous bulk queries (returned 0 kind:0). With primal as indexer, purplepag.es's AUTH-walling was partially masked.

## Trigger

User directed making primal an app relay instead of indexer. Empirical measurement showed adding a single broad app relay (nos.lol) takes resolution from 60.3% → 88.8% (+300 follows), and 204 of those 300 net-new are no-NIP-65 users structurally unreachable by the outbox model. Investigation revealed that removing primal from the indexer set would leave purplepag.es (AUTH-walled) as the sole dedicated indexer, which would silently break kind:10002 discovery — the outbox model would go inert again.

## Decision

(1) primal.net changed from both,indexer to both (app relay only) in nmp-chirp-config. (2) kind:10002 discovery probe in nmp-core recompile.rs now targets indexer_relays ∪ app_relays (deduplicated via BTreeSet), preventing regression when dedicated indexers are unavailable or AUTH-walled. Probe frames also routed through auth_gate.partition() to buffer on paused/AUTH-walled app relays. (3) relay.nostr.band purged from all 13 references across the codebase (tests, docs, gallery TUI bogus log line).

## Consequences

- App relays now participate in kind:10002 (relay-list) discovery, not just kind:0 (profile) queries
- AUTH-walled or absent dedicated indexers are no longer a single point of failure for the outbox model
- Measurement confirms 31.7% of follows have no NIP-65; only app relays can reach ~204 of those
- The nmp-core routing change (recompile.rs) raises a framework vs app-level boundary question — user flagged concern about routing policy living in the framework rather than app config
- 3 new kernel tests added: app_relay_only_still_emits_mailbox_probe, probe_unions_indexer_and_app_relays, no_indexer_no_app_relay_means_no_probe
- Web feed E2E test manually verified against #1448 (CI path-filters it out for nmp-core changes)

## Open Tail

- User must decide whether the nmp-core kind:10002 routing change stays or reverts (relay config is app-level regardless; the question is whether discovery routing policy belongs in the framework)
- Adding nos.lol or another broad app relay to Chirp's default config could push resolution from ~60% toward ~89% — offered but not yet directed
- On-device builds still on v0.8.0 (predates #1448/#1451); phone doesn't have the primal-as-app-relay change yet

## Evidence

- transcript lines 3191-3745
