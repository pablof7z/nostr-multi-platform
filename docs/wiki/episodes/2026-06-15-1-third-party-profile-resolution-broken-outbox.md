---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: active
subjects:
  - nmp-core-profile-resolution
  - outbox-model
  - chirp-ios-claim-coverage
supersedes: []
related_claims: []
source_lines:
  - 1-3177
captured_at: 2026-06-15T11:23:07Z
---

# Episode: Third-party profile resolution broken: outbox model inert for non-self authors

## Prior State

The outbox model (NIP-65 relay routing) existed in kernel code but was inert for third-party profile resolution. kind:10002 (relay lists) was only fetched for the self/active account at startup, never for arbitrary authors. Third-party kind:0 queries only hit indexer relays (Lane 6), bypassing the author's own write relays entirely. One of two indexers (purplepag.es) AUTH-walls anonymous queries, further reducing yield. Chirp iOS also only called claim_profile for a subset of visible pubkeys (not mentions, attributions, or reaction/repost authors).

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS, requesting root-cause investigation across iOS UI, NMP kernel, and relay configuration. Multi-agent investigation traced the kernel's claim_profile → profile_claim_request → route_outbox_subscription_relays path end-to-end and identified the MailboxCache gap as the fatal flaw.

## Decision

Shipped three kernel-level fixes as NMP v0.8.0: (1) Outbox kind:10002 discovery for arbitrary authors (not just self) — the kernel now fetches third-party NIP-65 relay lists so the outbox model actually routes kind:0 to the author's own relays. (2) Retry-on-miss for failed kind:0 lookups. (3) Liveness hint integration. Also fixed Chirp iOS to self-claim profiles for mentions, attributions, and names.

## Consequences

- Measured profile resolution improved from 10.2% → 50% → 60.3% (5× improvement)
- A web-feed infinite snapshot loop regression in wasm was caught during CI and fixed before merge
- The 10.2% baseline was inflated to 27.9% on re-measurement because purplepag.es stopped AUTH-walling — relay availability is a confounding variable
- 31.7% of follows publish no kind:10002 at all and remain structurally unreachable by the outbox model alone
- Proactive profile fetch on note ingest was previously deliberately removed (F-CR-00); the new approach is demand-driven (claim_profile) with outbox discovery

## Open Tail

- The ~40% of follows still unresolved are split between no-NIP-65 users (only reachable via app relays) and users whose relays are simply unreachable
- The nmp-core routing change (kind:10002 probe → indexer ∪ app_relays) is pending user's keep/revert decision on framework vs app-level boundary

## Evidence

- transcript lines 1-3177
