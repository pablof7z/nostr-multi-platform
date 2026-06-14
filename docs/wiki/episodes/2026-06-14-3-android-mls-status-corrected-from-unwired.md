---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - android-marmot
  - mls-support
supersedes: []
related_claims: []
source_lines:
  - 33-102
captured_at: 2026-06-14T09:35:38Z
---

# Episode: Android MLS status corrected from unwired to wired-with-gaps

## Prior State

Memory held that Android was unwired for MLS (V-109); the assumption was Android needed full MLS wiring from scratch.

## Trigger

Sonnet verification agent examined the codebase and found Android IS wired: JNI layer (nativeMarmotRegisterActive/nativeMarmotUnregister), marmot feature enabled in build.gradle, Kotlin bridge with generic dispatchAction path, KernelModel ops (createGroup, sendGroupMessage, publishKeyPackage, acceptWelcome, declineWelcome), and UI (GroupsScreen, GroupChatScreen). The gap is surface area, not architecture.

## Decision

Android MLS status upgraded from unwired to wired-with-gaps; V-109 is addressed. Missing ops are leave/invite/remove and their UI buttons — a surface-area completion task, not an architectural wiring task. Both platforms are SHOULD-WORK-UNVERIFIED (no cross-client interop test on CI).

## Consequences

- fix planning targets adding leave/invite/remove ops and UI to Android rather than wiring from scratch
- no live device interop test exists on CI for either platform; real verification requires emulator-to-simulator MLS round-trip
- NMP_MARMOT_MOCK_KEYRING=1 resolves the headless keychain blocker for CI testing

## Open Tail

- Android gap agents dispatched but not yet resolved in this session

## Evidence

- transcript lines 33-102

