---
type: episode-card
date: 2026-06-14
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: product
status: active
subjects:
  - android-identity
  - keystore-keyring
  - cold-start-restore
supersedes:
  - 2026-06-14-2-android-production-identity-persistence-keyring-capability
related_claims: []
source_lines:
  - 12373-12679
captured_at: 2026-06-14T08:49:50Z
---

# Episode: Android production identity persistence — keyring + restore must be unconditional, not DEBUG-gated

## Prior State

Android's `MainActivity.onCreate` only called `startWithContext()` (which installs `KeystoreKeyringCapability` + calls `identityRestore`) when `BuildConfig.DEBUG`-gated test extras were present. Production always took the bare `start()` path, leaving no keyring capability installed and no identity restore. The Rust kernel's persist/restore chain (`enqueue_persist_current_active_session` / `session_persistence::restore_active_session`) was already correct but wrote/read exclusively through the capability callback — which was never installed in production.

## Trigger

v1 feature-completeness audit discovered the gap: production users are logged out after every app restart because `identityRestore` and `KeystoreKeyringCapability` are never wired outside debug builds.

## Decision

Collapse `start()` + `startWithContext()` into a single unconditional path that installs `KeystoreKeyringCapability` and calls `identityRestore` for all builds (mirroring iOS, which registers the keychain capability in `init` and restores in `start()` unconditionally). Extract ordering into a pure JVM-testable `planKernelLaunch()` with a `BridgeLaunchSeam` production adapter. No Rust changed.

## Consequences

- PR #1392 merged (ecce4b716) — Android users now persist sign-in across cold restarts in production.
- JVM regression test (`KernelLaunchSequenceTest`) added proving: production path (null test args) installs keyring capability AND calls identityRestore; install precedes restore; start precedes restore.
- Dead write-only `keystoreKeyringCapability` field removed (JNI GlobalRef owns it once installed).
- The Rust kernel logic was already correct — the defect was purely in the Android shell wiring.
- Follow-list projection gap on Android/desktop (kernel emits `nmp.follow_list`, iOS consumes it via `FollowListStore`, Android/desktop don't) was documented in #1391 but not fixed in this session.

## Open Tail

- Next priority from #1391 gap-map: Android+desktop consume `nmp.follow_list` projection (mirrors iOS `FollowListStore`) — closes 3 gaps at once, but deferred to avoid KernelModel.kt collision with the identity fix.

## Evidence

- transcript lines 12373-12679

