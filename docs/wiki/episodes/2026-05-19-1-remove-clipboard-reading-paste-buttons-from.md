---
type: episode-card
date: 2026-05-19
session: 5d893073-9635-450b-b8e9-50648bc1a4e7
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5d893073-9635-450b-b8e9-50648bc1a4e7.jsonl
salience: root-cause
status: active
subjects:
  - chirp-onboarding
  - clipboard-access
supersedes: []
related_claims: []
source_lines:
  - 1-113
captured_at: 2026-06-18T04:20:28Z
---

# Episode: Remove clipboard-reading paste buttons from onboarding

## Prior State

Onboarding views had conditional Paste buttons that read UIPasteboard.general.string directly inside the SwiftUI body, causing the system to poll the clipboard on every state re-evaluation and triggering the iOS pasteboard permission toast on every app launch.

## Trigger

User reported: 'every time the app runs it tries to copy from the clipboard — probably for logging in'

## Decision

Removed the conditional paste-affordance blocks from OnboardingView+Components.swift (nsec field) and OnboardingView+NIP46.swift (bunker URI field) entirely. Users can still paste via the standard iOS text-field long-press menu. Button-action-level clipboard reads (WalletView paste button) were left intact — only body-level reads were the problem.

## Consequences

- iOS clipboard permission toast no longer appears on every app launch
- No dedicated paste affordance in onboarding fields; relies on system paste menu
- SwiftUI body should never read UIPasteboard directly — only inside button actions

## Open Tail

*(none)*

## Evidence

- transcript lines 1-113

