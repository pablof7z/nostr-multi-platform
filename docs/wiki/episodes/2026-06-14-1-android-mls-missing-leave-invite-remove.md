---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: product
status: active
subjects:
  - android-marmot-ops-gap
  - chirp-android-mls
supersedes:
  - 2026-06-14-3-android-mls-status-corrected-from-unwired
related_claims: []
source_lines:
  - 73-102
captured_at: 2026-06-14T20:54:15Z
---

# Episode: Android MLS missing leave/invite/remove operations vs iOS

## Prior State

V-109 was believed addressed — Android MLS/Marmot was considered wired and functional, comparable to iOS

## Trigger

Verification audit revealed Android KernelModel exposes only createGroup/sendGroupMessage/publishKeyPackage/acceptWelcome/declineWelcome but NOT leave, invite (on existing group), or remove; Android UI lacks leave/invite/remove buttons that iOS has

## Decision

Gap acknowledged — Android MLS is wired but operationally incomplete; the missing ops (leave, invite-to-existing-group, remove) must be added to KernelModel + UI to reach parity with iOS

## Consequences

- Android users cannot leave a group, invite to an existing group, or remove members — only create/join/send/accept/decline
- Rust FFI already supports all ops via generic dispatchAction — gap is Kotlin bridge + UI only, consistent with Rust-owns-logic doctrine
- Fix scope is bounded: add 3 KernelModel methods calling dispatchAction + 3 UI buttons in GroupChatScreen.kt

## Open Tail

- Fable→Sonnet→Opus→Haiku pipeline needed to plan, implement, review, and verify the missing ops
- Cross-client interop test (Android emulator ↔ iOS simulator group chat) still unvalidated

## Evidence

- transcript lines 73-102
