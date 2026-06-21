---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - nmp-android
  - wallet-status
  - is_connected
supersedes:
  - 2026-06-18-4-android-walletscreen-isconnected-derived-from-tone
related_claims: []
source_lines:
  - 194-412
  - 388-392
captured_at: 2026-06-18T20:12:30Z
---

# Episode: Native shells must bind Rust-owned state, not derive from discriminants

## Prior State

Android WalletScreen derived isConnected by branching on the wallet tone discriminant (walletTone != 'inactive'), causing errored wallets to incorrectly show a Disconnect button. iOS WalletView already used the Rust-computed WalletStatus.is_connected boolean.

## Trigger

Issue #1493 audit (P4 F2) identified the native-derives-from-discriminant pattern as a violation of Rust-owned source-of-truth.

## Decision

Android now binds the Rust-computed WalletStatus.is_connected boolean directly (already in schema + generated binding), matching iOS WalletView. Errored wallet correctly shows as not-connected.

## Consequences

- Errored wallet state now displays correctly on Android (not-connected instead of showing Disconnect).
- Establishes pattern: native shells bind Rust-computed booleans/discriminants verbatim rather than re-deriving UI state from wire-format fields.

## Open Tail

*(none)*

## Evidence

- transcript lines 194-412
- transcript lines 388-392

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-8-native-shells-must-bind-rust-owned.json`](transcripts/2026-06-18-8-native-shells-must-bind-rust-owned.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-8-native-shells-must-bind-rust-owned.json`](transcripts/raw/2026-06-18-8-native-shells-must-bind-rust-owned.json)
