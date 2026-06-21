---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - wallet-status
  - native-policy-drift
  - android-wallet
supersedes: []
related_claims: []
source_lines:
  - 194-412
  - 388-391
captured_at: 2026-06-18T19:42:43Z
---

# Episode: Android WalletScreen isConnected derived from tone discriminant instead of Rust-computed value

## Prior State

Android WalletScreen derived `isConnected = walletTone != "inactive"`, branching on a Rust discriminant. This caused errored wallets to incorrectly show a Disconnect button (the tone for error was not "inactive"). iOS WalletView already used the Rust-computed `WalletStatus.is_connected` bool.

## Trigger

#1493 audit finding P4 Finding 2; codex-design-first verified the fix.

## Decision

Bind the Rust-computed `WalletStatus.is_connected` bool verbatim on Android, matching iOS. Landed in PR #1530 (merge commit 7680dcf84).

## Consequences

- Errored wallets now correctly show as not-connected on Android
- Native policy (connection state derivation) no longer drifts from Rust source-of-truth
- Doctrine D7 gate passed in full CI

## Open Tail

*(none)*

## Evidence

- transcript lines 194-412
- transcript lines 388-391

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-4-android-walletscreen-isconnected-derived-from-tone.json`](transcripts/2026-06-18-4-android-walletscreen-isconnected-derived-from-tone.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-4-android-walletscreen-isconnected-derived-from-tone.json`](transcripts/raw/2026-06-18-4-android-walletscreen-isconnected-derived-from-tone.json)
