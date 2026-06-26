---
type: episode-card
date: 2026-06-26
session: 03e8696e-a094-44af-aa02-2d559b5265c1
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/03e8696e-a094-44af-aa02-2d559b5265c1.jsonl
salience: architecture
status: active
subjects:
  - m14-uniffi-migration
  - android-ffi
  - ffi-architecture
supersedes: []
related_claims: []
source_lines:
  - 156-183
captured_at: 2026-06-26T07:58:58Z
---

# Episode: M14 UniFFI strategy split: conditionally elevate Android binding generation to v1, defer iOS migration

## Prior State

M14 (full 167-symbol UniFFI migration) deferred post-v1 per plan.md; C-ABI layer hand-written (734-line header, ~9.1k Swift glue, bespoke Android JNI) but protected by ffi-drift.yml CI gate; consensus is not to pull M14 into v1

## Trigger

User challenges the hand-maintenance cost of three parallel FFI copies (Rust, C header, Swift, Android JNI) and questions whether the maintenance burden justifies pulling M14 forward; this prompts detailed cost-benefit analysis

## Decision

The maintenance-cost argument is valid. Strategy should split M14 into two tiers: (1) Android binding generation (generate Kotlin from shared Rust source) conditionally elevate to v1 if hand-JNI maintenance is causing acute pain; (2) iOS full migration (replace 167 hand-written symbols) remain deferred post-v1 to avoid ecosystem churn during v1 stabilization and re-spending sunk labor on a surface already protected by drift gate

## Consequences

- Android FFI surface becomes a v1 cost-benefit evaluation, with targeted Kotlin generation as potential scope if hand-JNI pain is acute
- iOS binding layer remains hand-written and drift-gated through v1 conclusion
- Ecosystem churn avoided; external consumers (podcast-player, win-the-day, hl) and internal products (Chirp, nmp-gallery) not disrupted during critical v1 convergence window
- Forward cost of adding new seams remains 3–4x hand-edits for iOS; Android surface gets generation parity if Android slice is selected
- Full 167-symbol homogeneous UniFFI migration deferred to post-v1

## Open Tail

- User has not formally committed to this two-tier strategy; decision is explicitly conditional on acute Android hand-JNI maintenance pain
- If Android binding generation selected for v1, would require independent ADR and effort estimate for scope negotiation

## Evidence

- transcript lines 156-183

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-1-m14-uniffi-strategy-split-conditionally-elevate.json`](transcripts/2026-06-26-1-m14-uniffi-strategy-split-conditionally-elevate.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-1-m14-uniffi-strategy-split-conditionally-elevate.json`](transcripts/raw/2026-06-26-1-m14-uniffi-strategy-split-conditionally-elevate.json)
