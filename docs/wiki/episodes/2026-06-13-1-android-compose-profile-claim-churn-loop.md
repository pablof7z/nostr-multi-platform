---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: root-cause
status: active
subjects:
  - android-claim-churn
  - compose-profile-host
  - nostr-avatar
  - nostr-profile-name
  - vendored-component-drift-gate
supersedes: []
related_claims: []
source_lines:
  - 7462-7541
captured_at: 2026-06-13T19:26:59Z
---

# Episode: Android Compose profile-claim churn loop — same bug class as chirp-web

## Prior State

PR #1294 introduced Compose profile components where DisposableEffect was keyed on profileHost, and rememberKernelProfileHost was keyed on (model, profiles). Since the profiles map changes identity every snapshot tick, profileHost was recreated every tick, causing both NostrAvatar and NostrProfileName to release and re-claim on every snapshot — an infinite churn loop that prevented profile names/avatars from ever stabilizing. This was the identical bug class chirp-web already fixed in commit 4d1888f9a.

## Trigger

Focused audit of genuinely-new #1294 code in a documented-bug-prone area (profile claim lifecycle) discovered the same churn pattern chirp-web had already fixed.

## Decision

Two-part fix applied: (a) remove profileHost from DisposableEffect key lists in NostrAvatar.kt and NostrProfileName.kt; (b) stabilize rememberKernelProfileHost to remember(model) only, threading the latest profiles map through a profilesProvider lambda backed by rememberUpdatedState. A non-@Composable resolve() helper was extracted so the stability contract is unit-testable without Robolectric/Compose-test. Fix applied identically to both canonical registry and vendored Android copies (byte-identical drift gate constraint).

## Consequences

- Profile names/avatars now stabilize correctly on Android — the churn loop is broken.
- Any future edit to the Compose profile-component family must touch both crates/nmp-cli/registry/compose/ and android/app/src/main/java/org/nmp/android/components/ identically (drift gate enforces byte-identity).
- @Composable profile functions cannot be unit-tested from plain JUnit; future Android claim-lifecycle tests must target resolve()-style non-composable helpers.
- #1302's component edits left registry.json stale, breaking master (fixed in #1313) — establishing that nmp export jsrepo regen is mandatory after canonical component changes.

## Open Tail

- DmConversationListScreen double-collects model.state independently of parent (consistency hazard, filed #1303).
- ThreadScreen does not provide LocalProfileClaimer — thread author names never trigger on-demand kind:0 fetch (functional gap, filed #1303).

## Evidence

- transcript lines 7462-7541

