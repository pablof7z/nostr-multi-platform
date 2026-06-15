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
  - kind0-fetch
  - claim-registry
  - liveness-hint
supersedes:
  - 2026-06-15-1-third-party-profile-resolution-indexer-only
related_claims: []
source_lines:
  - 1-44
  - 49-84
captured_at: 2026-06-15T04:23:17Z
---

# Episode: Third-party profile resolution overhaul: outbox model was inert, registry + discovery + liveness adopted

## Prior State

~50% of pubkeys never resolved because the kernel only queried indexer relays (purplepag.es) for kind:0. The outbox model machinery existed but was inert for third-party profiles: kind:10002 was only fetched for the self/active account at startup (SELF_KINDS_TAILING), so MailboxCache had no entries for arbitrary authors. Profile claims used bespoke bypass paths with no unified chokepoint. No liveness distinction — all claims treated identically. No retry-on-miss. Probed-mailboxes re-arm cleared on every relay connect (even reconnects to already-connected relays). iOS UI only claimed profiles for feed avatars, not for mentions, reply attribution, or standalone names.

## Trigger

User reported ~50% of pubkeys fail to resolve in Chirp iOS and demanded root-cause investigation. Multi-agent analysis confirmed the outbox model gap: Lane 1 (author's NIP-65 write relays) empty for third-party pubkeys → queries fall through to operator indexers only → users who published kind:0 only to their own relays silently never resolve.

## Decision

Full profile-claim registry migration replacing the bespoke path: (a) unified claim chokepoint via register_profile_claim_interest + recompile_and_diff_with_lookup; (b) proactive kind:10002 D3 discovery probe for third-party pubkeys (Nip65Arrived triggers re-route to author's write relays); (c) liveness hint distinguishing CacheOk (feed-row avatars, one-shot reverify) vs Live (profile screen, tailing) via 5-arg nmp_app_claim_profile FFI; (d) retry-on-miss for failed lookups; (e) probed-mailboxes re-arm gated to genuine reconnects (indexer_socket_was_down), not every connect event; (f) iOS self-claiming surfaces for mentions, reply attribution, standalone names.

## Consequences

- Breaking FFI change: nmp_app_claim_profile 4→5 args (added liveness: c_int); all consumer apps must adapt
- Version cut 0.7.2→0.8.0 (semver breaking)
- ~50% unresolved pubkey rate expected to drop dramatically (users publishing kind:0 only to own relays now discoverable)
- Profile resolution is now progressive/never-blank: fallback indexer first, then author relays once 10002 arrives
- Consumer apps (podcast-player, hl, tenex-off) required pin update + 5th-arg adaptation; win-the-day confirmed as non-NMP consumer
- Docs PR (#1439) authored capturing the new internals as authoritative reference

## Open Tail

- NIP-60 (wallet) profile resolution not yet addressed (issue #1434 filed)
- Actual resolution-rate improvement not yet measured in production (baseline measured at ~10% kernel-side / ~50% iOS-side before fix)

## Evidence

- transcript lines 1-44
- transcript lines 49-84
