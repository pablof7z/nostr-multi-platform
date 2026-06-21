---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: active
subjects:
  - wallet-screen
  - follow-persistence
  - native-policy
supersedes:
  - 2026-06-18-8-native-shells-must-bind-rust-owned
related_claims: []
source_lines:
  - 2027-2028
captured_at: 2026-06-19T11:51:39Z
---

# Episode: Native policy duplication → Rust-bound; no-feed-after-signin bug fixed

## Prior State

WalletScreen duplicated Rust policy in native code; a latent bug caused empty feed after sign-in on both iOS and Android platforms.

## Trigger

#1493 audit P4 finding: native code holding policy that should live in Rust.

## Decision

WalletScreen bound to Rust as source of truth; follow-list persistence fixed kernel-side so feed populates correctly after sign-in.

## Consequences

- Feed now populates after sign-in on both platforms
- 2 PRs merged: #1530 (WalletScreen), #1545 (follow-feed kernel persist)
- Web follow-up filed (#1546) for web config single-source + cache→wasm

## Open Tail

- #1546 — web config single-source + ProjectionMergeCache into wasm worker

## Evidence

- transcript lines 2027-2028

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-7-native-policy-duplication-rust-bound-no.json`](transcripts/2026-06-19-7-native-policy-duplication-rust-bound-no.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-7-native-policy-duplication-rust-bound-no.json`](transcripts/raw/2026-06-19-7-native-policy-duplication-rust-bound-no.json)
