---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - chirp-profile-resolution
  - nmp-kernel-claim-profile
  - outbox-model-nip65
  - logical-interest-registry
supersedes:
  - 2026-06-14-1-third-party-profile-resolution-inert-outbox
related_claims: []
source_lines:
  - 1-4
  - 48-84
  - 86-165
  - 1596-1638
  - 2003-2013
  - 1815-1849
captured_at: 2026-06-14T23:29:29Z
---

# Episode: Profile resolution root cause: claim_profile bypasses registry, outbox model inert for third-party profiles

## Prior State

Chirp iOS showed ~50% of users with unresolved pubkeys. The kernel had the outbox model (NIP-65 write-relay routing) fully implemented but it was inert for third-party profile resolution: claim_profile used bespoke route_outbox_subscription_relays + req_for_relay instead of registering a LogicalInterest, so it never got the D3 kind:10002 probe or Nip65Arrived re-route. kind:0 queries were sent only to operator indexer relays. Additionally, purplepag.es (one of two indexers) is AUTH-walled and returns 0 to anonymous queries, meaning effectively only primal.net served profiles at all.

## Trigger

User directive to investigate why ~50% of pubkeys don't resolve; multi-agent investigation across relay configuration, kernel acquisition path, UI claim coverage, and outbox model revealed the fatal gap: MailboxCache has no entry for third-party authors because kind:10002 is only fetched for the self/active account at startup (startup.rs SELF_KINDS_TAILING), so Lane 1 (outbox) is always empty for strangers.

## Decision

Migrate claim_profile and drain_pending_reverify to the LogicalInterest registry path (using claim_event as the proven reference implementation), so third-party profiles inherit the D3 10002 probe + outbox routing + Nip65Arrived re-route. Add liveness hint to FFI (CacheOk=OneShot for feed avatars, Live=Tailing for profile screens). Implementation in kernel PR #1436.

## Consequences

- Baseline measurement confirms ~5× improvement: 10.2% → 50.0% profile resolution (+420 profiles out of 1054 follows)
- Full codebase audit confirmed the bespoke-REQ anti-pattern is isolated to the profile-claim family; all other paths (claim_event, NIP-17 DMs, NIP-57 zaps, reactions, contacts, follows) correctly use the registry
- drain_pending_reverify (F-TTL re-verification) has the same root defect and must be migrated alongside claim_profile as one coherent unit — the M2 scope was expanded to include it
- purplepag.es AUTH-wall means the current indexer-only path effectively relies on primal.net alone — outbox routing is even more critical than expected
- FFI signature changed from 4 to 5 args: nmp_app_claim_profile now takes liveness param (0=CacheOk, non-zero=Live); all in-repo callers (nmp-ffi, nmp-android-ffi, nmp-wasm, apps, ffi-stress) swept
- Warm-reclaim of already-cached profiles is zero-REQ (CacheOk claim of a resident profile registers no network interest)
- nip60 hardcoded purplepag.es relay is a separate minor follow-up (filed as GitHub issue #1434)

## Open Tail

- Web Playwright E2E regression on PR branch (feed.spec.ts 'renders after connect' fails twice while green on 8 consecutive master runs) — investigating before merge, cannot assume flake
- Profile resolution ceiling at ~50% due to only 608/1054 follows publishing NIP-65 relay lists; users without kind:10002 still rely on indexer-only discovery
- Registry/gallery ProfileLiveness propagation not yet folded in (Chirp adopted liveness; other consumer apps still on 2-arg convenience default)

## Evidence

- transcript lines 1-4
- transcript lines 48-84
- transcript lines 86-165
- transcript lines 1596-1638
- transcript lines 2003-2013
- transcript lines 1815-1849
