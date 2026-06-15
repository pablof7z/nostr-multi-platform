---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: active
subjects:
  - nmp-profile-resolution
  - nmp-outbox-model
  - nmp-kernel
supersedes:
  - 2026-06-15-1-third-party-outbox-model-was-inert
related_claims: []
source_lines:
  - 1-83
  - 3160-3177
captured_at: 2026-06-15T10:39:31Z
---

# Episode: Outbox model was inert for third-party profile resolution — fixed in v0.8.0

## Prior State

The outbox model (NIP-65) existed in the router but was never used for resolving third-party profiles. kind:10002 (relay lists) was only fetched for the active/self account at startup. Third-party kind:0 queries went only to indexer-role relays (primal + purplepag.es). purplepag.es AUTH-walls anonymous queries, so only primal served profiles → ~10% baseline resolution for follows' kind:0.

## Trigger

User reported ~50% of pubkeys never resolved in Chirp iOS. Multi-agent investigation traced the root cause: MailboxCache had no entries for third-party authors → Lane 1 (outbox) empty → fell through to Lane 6 (indexer-only). The kernel never proactively fetched kind:10002 for arbitrary pubkeys — only for self at startup (startup.rs SELF_KINDS_TAILING).

## Decision

v0.8.0 overhauls profile resolution: kernel now fetches kind:10002 for third-party pubkeys (outbox discovery probe), adds retry-on-miss for failed lookups, and adds a liveness hint (CacheOk/Force) to claim_profile (4→5 args, breaking FFI change). All four consuming apps updated (Chirp, Highlighter, tenex-off, podcast-player). Measured: 10.2% → 50.0% → 60.3% resolution of follows' kind:0.

## Consequences

- FFI breaking change: nmp_app_claim_profile gained a 5th liveness argument; all consumers adapted (CacheOk for list/feed rows, Force for dedicated profile screens)
- Outbox model is now active for third-party profiles — kind:10002 discovery probe routes to indexers, and kind:0 queries route to each author's own write relays
- Resolution ceiling depends on relay availability (purplepag.es being AUTH-walled dropped baseline from ~28% to ~10% in one measurement)
- Adding a single broad app relay (nos.lol) pushes resolution to ~89% by covering the ~32% of follows with no NIP-65 at all — structurally unreachable by outbox alone

## Open Tail

- The ~39% still unresolved are follows who publish neither kind:10002 nor kind:0 to any of the queried relays — an app relay like nos.lol covers most of these

## Evidence

- transcript lines 1-83
- transcript lines 3160-3177
