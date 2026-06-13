---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: product
status: superseded
subjects:
  - mls-marmot
  - chirp-ios
  - chirp-android
  - nmp-marmot-ffi
supersedes: []
related_claims: []
source_lines:
  - 1-5
  - 33-101
  - 4577-4596
  - 4612-4697
  - 5010-5055
captured_at: 2026-06-13T18:51:06Z
---

# Episode: MLS end-to-end validated on both platforms — Rust-owns-all architecture confirmed

## Prior State

MLS on iOS and Android was unverified; V-109 (Android marmot gap) was believed possibly still open; no cross-client interop test existed on CI; the architecture goal (Rust owns all logic, apps are thin shells) was aspirational but unproven on real devices

## Trigger

Explicit directive to verify MLS works with no hacks, proper Rust-owned architecture, and apps need no heavy lifting; live device testing (iOS simulator ↔ Android emulator over relay.primal.net) surfaced real failures

## Decision

MLS is validated end-to-end on both platforms; architecture is confirmed sound — Opus review found zero protocol logic in Swift/Kotlin (only ADR-0032 display formatting). Apps get MLS through one generic dispatch_action("nmp.marmot") seam + projections. All MLS ops (create group, invite, join, send, receive, key package publish) live in nmp-marmot Rust crate.

## Consequences

- 6 PRs merged fixing real bugs found by live testing: #1227 (key-package autopublish parity for nsec sign-ins), #1237 (kernel PushInterest/EnsureInterest must serve store-first — the cause of iOS group-create silently no-op'ing), #1230/#1235 (deferred completion for key-package-gated ops + failures surfaced in snapshot), #1239 (shells stop treating 'submitted' as 'succeeded'), #1219 (Android --features marmot build was broken & uncaught by CI — added CI gate), #1261 (post-restart per-group kind:445 subscriptions never re-pushed)
- register_with_keys has an asymmetry: it re-pushes giftwrap inbox interest on restart but not per-group message interests; #1261 added resubscribe_all_groups to the register tail, reusing the existing cache_group_relays choke point
- The #1261 fix also replays stored-but-unprocessed 445s on restart via push_interest_and_serve (bonus from #1237's store-serve)
- Android mock keyring replaced with proper host-backed Keystore; NMP_MARMOT_MOCK_KEYRING=1 still available for headless CI
- Interest-withdrawal on unregister/account-switch has no seam yet (push_interest exists but remove_interest does not); group interests linger until process exit on account switch
- Android x86_64 ABI marmot build blocked on upstream openssl-src/NDK issue (#1218); arm64 works

## Open Tail

- Interest-withdrawal asymmetry on unregister/account-switch needs a remove_interest seam
- No cross-client interop test on CI (real_relay_marmot_roundtrip.rs is #[ignore])
- AI architecture signoff CI gate is red repo-wide due to invalid OpenAI API key (401)

## Evidence

- transcript lines 1-5
- transcript lines 33-101
- transcript lines 4577-4596
- transcript lines 4612-4697
- transcript lines 5010-5055

