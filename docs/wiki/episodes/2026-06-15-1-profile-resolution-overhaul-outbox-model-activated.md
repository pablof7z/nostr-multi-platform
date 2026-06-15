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
  - nmp-kernel
  - nmp-ffi
  - chirp-ios
supersedes:
  - 2026-06-15-1-third-party-profile-resolution-overhaul-outbox
  - 2026-06-15-2-ios-ui-missing-profile-claims-for
related_claims: []
source_lines:
  - 48-85
  - 2566-2568
captured_at: 2026-06-15T04:33:40Z
---

# Episode: Profile resolution overhaul: outbox model activated for third-party pubkeys via registry chokepoint

## Prior State

Profile resolution used a bespoke profile_claim_request path that only queried operator/indexer relays (e.g. purplepag.es). The outbox model (NIP-65 kind:10002) machinery existed in the router but was inert for third-party profiles — kind:10002 was only fetched for the self/active account at startup, so MailboxCache had no entries for arbitrary pubkeys, causing Lane 1 to be empty and fall through to indexer-only resolution. iOS UI components (mentions, reply attributions, standalone names) did not self-claim profiles at all. ~50% of users (those who only published kind:0 to their own relays) never resolved.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS and demanded root-cause investigation across kernel/UI/relay layers. Multi-agent investigation traced the fatal gap: the kernel never proactively fetches kind:10002 for third-party pubkeys, making the outbox model effectively dead for profile resolution.

## Decision

Migrated profile resolution from the bespoke path to a registry chokepoint (register_profile_claim_interest → recompile_and_diff_with_lookup). Added batched kind:10002 (D3) discovery probe in the recompile path so the outbox model actually resolves third-party write relays. Added liveness hint to claim_profile FFI (4→5 args: CacheOk for feed/inline surfaces, Live for profile screen → determines OneShot vs Tailing subscription). Made iOS UI surfaces self-claim (feed avatars, mentions, reply attributions, standalone names → CacheOk; profile screen → Live). Added retry-on-miss and gated indexer reprobe to genuine reconnects (indexer_socket_was_down) instead of clearing on every connect.

## Consequences

- FFI breaking change: nmp_app_claim_profile went 4→5 args, requiring all consumer apps to pass liveness (0=CacheOk, nonzero=Live)
- Version cut to 0.8.0 due to breaking C-ABI change
- Resolution rate improved from ~10% (baseline measurement) for feed scroll, with full outbox coverage expected to resolve the ~50% symptom
- iOS PR #1437 restores ABI consistency (NmpCore.h 5-arg + KernelBridge liveness wiring)
- Consumer apps (tenex-off, podcast-player, hl, chirp) all needed pin updates and FFI adaptation
- win-the-day confirmed as not an NMP consumer (pure-Swift Nostr), removed from upgrade list

## Open Tail

- NIP-60 wallet issue filed (#1434) as follow-up discovered during investigation
- Actual resolution rate post-install not yet measured on device
- Apps installed to iPhone but not launched/verified visually yet

## Evidence

- transcript lines 48-85
- transcript lines 2566-2568
