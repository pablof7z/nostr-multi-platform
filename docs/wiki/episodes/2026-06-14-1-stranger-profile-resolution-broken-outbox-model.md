---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - profile-resolution
  - outbox-model
  - claim-profile
  - logical-interest-registry
supersedes:
  - 2026-06-14-2-m2-migration-profile-claims-onto-generic
related_claims: []
source_lines:
  - 1-44
  - 73-82
  - 1027-1049
  - 1057-1192
  - 1597-1638
captured_at: 2026-06-14T22:00:46Z
---

# Episode: Stranger profile resolution broken — outbox model inert for third-party pubkeys, claims must migrate to registry path

## Prior State

Profile claims used a bespoke REQ-building path (profile_claim_request → route_outbox_subscription_relays → req_for_relay) that bypassed the LogicalInterest registry. The outbox model existed in the router substrate but was inert for third-party profiles: kind:10002 (NIP-65 relay lists) were only fetched for the self/active account at startup (startup.rs SELF_KINDS_TAILING=[0,3,10002,…] — self only). Stranger kind:0 queries went only to operator indexer relays (purplepag.es), so ~50% of users whose kind:0 was not pushed there silently never resolved. An explicit 'kind:0 must not leak to content relays' contract existed in profile.rs comments. All claims were treated identically with no liveness/freshness distinction.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS; multi-agent investigation traced the root cause through kernel, router, and iOS layers, confirming the bespoke path never triggers D3 10002 discovery for strangers and has no Nip65Arrived re-route. The outbox model's MailboxCache is empty for third-party pubkeys, so Lane 1 (write relays) is empty and queries fall to Lane 6 (indexer-only).

## Decision

Migrate claim_profile/release_profile to register LogicalInterest{kinds:[0], authors:[P], limit:None} through the refcounting registry (same path claim_event already uses — the proven-correct sibling). Delete the bespoke path: profile_claim_request, pending_profile_claim_requests, ProfileRequestState, refresh_profile_after_mailbox, and the obsolete indexer-only contract comments. Add a client-hintable liveness FFI param (CacheOk→OneShot for feed avatars, Live→Tailing for ProfileView; Tailing wins when mixed). Make probed_mailboxes epoch-gated for retry-on-miss (epoch bumps on indexer reconnect + new-indexer-added). Seed nprofile relay hints into claim interests so URI-originated claims can resolve authors with no indexer 10002.

## Consequences

- Stranger profiles now inherit the D3 10002 probe, set-cover relay minimization, progressive re-route, and nprofile hints from the single recompile chokepoint — closing the ~50% resolution gap
- The 'kind:0 must not leak to app relays' contract is obsolete; kind:0 claims use default generic routing (app_relays + indexer when cold, author write relays when warm)
- drain_pending_reverify (F-TTL, requests/mod.rs:327) has the same bespoke bypass — same silent-miss for uncached-10002 authors during stale re-verifies — and must be migrated in the same kernel PR
- One new FFI param (liveness: c_int) on nmp_app_claim_profile; feed avatars pass CacheOk, ProfileView passes Live; Tailing upgrades OneShot when mixed claims exist on the same pubkey
- refresh_profile_after_mailbox is deleted — Nip65Arrived recompile now handles the re-route automatically for registered interests
- All other subsystems (claim_event, NIP-17 DMs, NIP-57 zaps, reactions, contacts, follow-feed, browse) confirmed clean via full codebase audit — the bypass was isolated to the profile-claim family
- NIP-60 hardcoded purplepag.es in nip60/relay.rs is a separate minor follow-up (off-actor wallet code, not part of this migration)

## Open Tail

- PR structure/sequencing still undecided (iOS Fault-A, kernel M2 migration, probe retry — fold or split for reviewability)
- Whether Tailing ProfileView subs need teardown guarantees for deep navigation stacks
- Whether probe-epoch triggers (reconnect + new-indexer) are sufficient or a TTL/periodic component is also needed
- NIP-60 hardcoded purplepag.es relay pin to be filed as separate tracked issue

## Evidence

- transcript lines 1-44
- transcript lines 73-82
- transcript lines 1027-1049
- transcript lines 1057-1192
- transcript lines 1597-1638
