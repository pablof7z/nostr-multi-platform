---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: product
status: active
subjects:
  - mls-marmot
  - chirp-ios
  - chirp-android
supersedes:
  - 2026-06-13-1-mls-end-to-end-validated-on
related_claims: []
source_lines:
  - 1-5
  - 33-108
  - 5029-5055
captured_at: 2026-06-13T20:13:27Z
---

# Episode: MLS cross-platform validated end-to-end on real devices

## Prior State

MLS on Android was tracked as unwired (V-109); iOS was 'should-work-unverified'. No live device interop test existed. Seven latent bugs prevented actual cross-platform MLS group messaging from working.

## Trigger

User directive to verify MLS works with proper architecture (Rust owns all logic, apps are thin UI shells) and fix any gaps through the full agent pipeline.

## Decision

MLS confirmed working end-to-end (iOS ↔ Android, real relay, encrypted group messaging with post-restart survival). Seven bugs found and fixed via merged PRs: nsec sign-ins never auto-published key packages (#1227), kernel never served store-first on PushInterest (#1237 — root cause of iOS group-create silently doing nothing), key-package-gated ops failed terminally with no retry (#1230/#1235), shells treated 'dispatch submitted' as 'succeeded' (#1239), Android --features marmot build broken and uncaught by CI (#1219), post-restart groups never re-subscribed to live messages (#1261).

## Consequences

- Architecture confirmed sound: all MLS logic in Rust, apps are thin shells with zero protocol logic (only ADR-0032 display formatting)
- Post-restart resubscription root cause: register_with_keys only pushed giftwrap inbox interest, not per-group interests; Inner.group_relays is an in-memory HashMap lost on restart. Fix: resubscribe_all_groups() enumerates persisted groups, re-establishes per-group kind:445 subscriptions via existing cache_group_relays choke point, also replays stored 445s via push_interest_and_serve
- Interest-withdrawal asymmetry identified: nmp_marmot_unregister does not withdraw per-group 445 interests; no remove_interest seam exists. On account-switch, prior account's group interests linger until process exit
- Android x86_64 marmot build blocked on upstream openssl-src/NDK issue (#1218); arm64 works
- Live device re-test confirmed post-fix: B's message arrived and decrypted on relaunched iOS A with no nudge

## Open Tail

- Interest-withdrawal seam needed for account-switch correctness
- Android x86_64 build unblocked on upstream openssl-src fix

## Evidence

- transcript lines 1-5
- transcript lines 33-108
- transcript lines 5029-5055

