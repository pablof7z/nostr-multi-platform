---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-core-profile-resolution
  - outbox-model
  - kind-0-fetch
  - claim-profile
supersedes:
  - 2026-06-15-1-outbox-model-activated-for-third-party
related_claims: []
source_lines:
  - 48-83
captured_at: 2026-06-15T10:28:02Z
---

# Episode: Third-party outbox model was inert for profile resolution

## Prior State

The kernel had full outbox/NIP-65 router machinery (MailboxCache, lane-based relay routing), but kind:10002 (relay lists) was only fetched for the self/active account at startup. For any third-party pubkey, MailboxCache had no entry, so Lane 1 was always empty, and kind:0 queries fell through to indexer-lane relays only (purplepag.es + primal). Purplepag.es AUTH-walls anonymous queries, so ~50% of pubkeys never resolved.

## Trigger

User reported ~50% of pubkeys in Chirp iOS never resolve and asked for root-cause investigation. Multi-agent trace of the kernel code path (profile.rs, router.rs, startup.rs) revealed the MailboxCache gap: the outbox model was implemented but never activated for third-party profiles because their kind:10002 was never fetched.

## Decision

Added proactive kind:10002 fetch on claim_profile (so third-party relay lists are discovered before routing kind:0 queries), retry-on-miss for kind:0, and a liveness hint parameter to the claim_profile FFI (4→5 args). Shipped as nmp v0.8.0 (PRs #1436, #1437, #1438).

## Consequences

- Follows' kind:0 resolution rose from ~10% (indexer-only) to 50–60% (outbox model active)
- Breaking FFI change: claim_profile 4→5 args required all consumer apps (chirp, podcast-player, tenex-off, hl) to be updated
- nmp-app-template crate renamed to nmp-defaults (ADR-0046), nmp_app_free_string → nmp_free_string — additional breakage surfaced during consumer upgrades
- web-feed infinite snapshot loop regression in wasm hit CI and was fixed before merge (same recompile.rs area)
- Profile resolution is still capped at ~60% by protocol realities: ~32% of follows publish no kind:10002, requiring app relays to reach them

## Open Tail

- ~40% of follows still don't resolve — the no-NIP-65 cohort needs app-relay coverage
- nip60/zap follow-up issue filed (#1434)

## Evidence

- transcript lines 48-83
