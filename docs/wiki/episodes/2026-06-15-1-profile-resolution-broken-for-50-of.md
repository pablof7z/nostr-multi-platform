---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - profile-resolution
  - outbox-model
  - claim-profile
  - kind-0
  - kind-10002
supersedes:
  - 2026-06-14-1-profile-resolution-must-use-registry-path
related_claims: []
source_lines:
  - 1-5
  - 48-85
  - 1596-1639
  - 1815-1850
  - 2000-2015
captured_at: 2026-06-15T00:22:07Z
---

# Episode: Profile resolution broken for ~50% of users — outbox model inert for third-party kind:0

## Prior State

claim_profile used bespoke routing (route_outbox_subscription_relays + req_for_relay), querying only operator/indexer relays for kind:0. The kernel never proactively fetched kind:10002 (NIP-65 relay lists) for third-party pubkeys, so the outbox model's Lane 1 (author's write relays) was always empty for strangers. Profiles only resolved if purplepag.es or primal.net happened to index them. ~50% of pubkeys never resolved. The proactive profile fetch on note ingest had been deliberately removed (F-CR-00), making UI claims the sole trigger.

## Trigger

User reported that ~50% of pubkeys never resolve in Chirp iOS and asked for root-cause investigation. Multi-agent audit revealed: (1) claim_profile bypasses the InterestRegistry, getting no D3 kind:10002 probe and no Nip65Arrived re-route; (2) drain_pending_reverify has the same bespoke bypass; (3) baseline measurement showed only 10.2% resolution on indexer-only path (108/1054 follows), with purplepag.es returning 0 results anonymously (NIP-42 AUTH-walled), leaving primal.net as the sole effective indexer.

## Decision

Migrate claim_profile and drain_pending_reverify to the registry path (LogicalInterest registration), inheriting the D3 kind:10002 probe, outbox routing, set-cover, and Nip65Arrived re-route — using claim_event as the proven reference implementation. Add nprofile relay hints via claim_profile_with_hints and probe-epoch retry on indexer reconnect. Delete the bespoke code (profile_claim_request, pending_profile_claim_requests, ProfileRequestState, profile_requests, refresh_profile_after_mailbox, relay_lifecycle re-queue). The entire codebase audit confirmed only these two paths (plus minor nip60 hardcoded relay) use the anti-pattern; all other subsystems (claim_event, DMs, zaps, reactions, contacts, follows) are clean.

## Consequences

- Baseline measurement: 10.2% → 50.0% profile resolution (~5× improvement, +420 profiles out of 1054 follows)
- Resolution ceiling is 57.6% (608/1054 follows publish NIP-65 relay lists); remaining ~43% require fallback-to-app-relay or other discovery
- purplepag.es is AUTH-walled (returns 0 kind:0 anonymously via NIP-42), making current path single-relay (primal.net only)
- drain_pending_reverify folded into the same migration as claim_profile — same subsystem (cold fetch + refresh of replaceable identity)
- nip60 hardcoded purplepag.es pin filed as separate minor follow-up (issue #1434)
- Kernel PR #1436 open, all core tests green (1541 + 113 + 60); web Playwright test under investigation (potentially real regression, not flake)
- FFI signature changed from 4-arg to 5-arg (liveness parameter); all callers across nmp-core, nmp-ffi, nmp-android-ffi, nmp-wasm swept

## Open Tail

- Web Playwright test (feed.spec.ts:24 'renders after connect') fails twice on PR branch while green on master — investigation agent dispatched, merge gated on verdict
- iOS PR held until kernel PR merges; wiring complete (KernelBridge.swift, NostrProfileHost.swift, call sites) and compiling clean
- Version cut + consumer-app updates + device installs queued behind merge

## Evidence

- transcript lines 1-5
- transcript lines 48-85
- transcript lines 1596-1639
- transcript lines 1815-1850
- transcript lines 2000-2015
