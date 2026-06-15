---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - profile-resolution
  - outbox-model
  - nmp-core
  - kind10002
  - claim-profile
supersedes:
  - 2026-06-15-1-profile-resolution-overhaul-outbox-discovery-for
related_claims: []
source_lines:
  - 1-5
  - 48-83
  - 2566-2568
  - 3160-3177
captured_at: 2026-06-15T09:21:42Z
---

# Episode: Third-party profile outbox resolution — kind:10002 discovery + registry chokepoint

## Prior State

The outbox model (NIP-65) routing machinery existed in the router but was inert for third-party profiles. kind:10002 relay lists were only fetched for the self/active account at startup; the kernel never proactively fetched kind:10002 for arbitrary authors. kind:0 queries went only to operator/indexer relay sets (Lane 6). The bespoke profile_claim_request path bypassed the registry/recompile chokepoint entirely. purplepag.es (the indexer) was AUTH-walled for anonymous connections. No retry-on-miss for failed lookups; probed_mailboxes was cleared on every reconnect rather than gated on genuine reconnect. Result: only ~10% of follows' kind:0 resolved.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS; multi-agent investigation traced root cause to the outbox model being implemented but never activated for third-party profiles (MailboxCache empty → Lane 1 empty → falls to indexer-only Lane 6), plus the profile-claim path bypassing the registry.

## Decision

Migrated all profile claims through the registry chokepoint (register_profile_claim_interest → recompile → subscription rebuild). Added kind:10002 D3 probe for authors whose relay lists are unknown, triggering Nip65Arrived re-route to their own NIP-65 write relays. Added retry-on-miss with probed_mailboxes re-arm gating only on genuine reconnect (indexer_socket_was_down), not every connect. Added liveness hint to claim_profile (5-arg C-ABI: CacheOk for feed/list avatars, Live for profile screens → Tailing subscription). Removed the bespoke profile_claim_request bypass.

## Consequences

- kind:0 resolution improved from ~10% to ~50% (~5×)
- Breaking C-ABI change: nmp_app_claim_profile 4→5 args; all consumers must adapt
- Web/wasm dispatch arm required separate snapshot-on-claim fix to avoid SolidJS remount loop
- F-CR-00 (proactive fetch removal) now safe because registry-driven claims cover the pipeline
- nmp-v0.8.0 released with the overhaul
- Remaining ~50% unresolved likely due to relay connectivity/liveness, not architecture

## Open Tail

- nip60 (NIP-60 wallet) issue filed (#1434) as separate follow-up discovered during investigation
- Consumer app branches (podcast-player, tenex-off, hl) not yet PR'd to their repos
- Ceiling above 50% may require further relay-liveness or outbox-fallback improvements

## Evidence

- transcript lines 1-5
- transcript lines 48-83
- transcript lines 2566-2568
- transcript lines 3160-3177
