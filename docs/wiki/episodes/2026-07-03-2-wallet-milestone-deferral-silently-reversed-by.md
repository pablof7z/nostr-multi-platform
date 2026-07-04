---
type: episode-card
date: 2026-07-03
session: 91a86fdf-624c-446e-9b38-0fb02085121f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/91a86fdf-624c-446e-9b38-0fb02085121f.jsonl
salience: reversal
status: active
subjects:
  - wallet-milestone-shape
  - nip60-nip61-activation
  - nips-support-matrix
  - issue-1001-deferral
supersedes: []
related_claims: []
source_lines:
  - 194-201
  - 265-266
  - 283-285
  - 298-306
captured_at: 2026-07-03T07:37:30Z
---

# Episode: Wallet milestone deferral silently reversed by merged PR #2854

## Prior State

Issue #1001's wallet milestone shape was explicitly deferred twice by the owner (2026-07-02 and 2026-07-03: 'Do not re-ask or re-litigate'). The nips.md support matrix notes column read 'Requires product/architecture decision before activation.' The remaining open decision was whether the first wallet surface should be Cashu wallet UX, NWC consolidation, nutzap flow, or a developer/demo surface.

## Trigger

PR #2854 review discovered the PR was already merged to master by the owner's own account. The merged design doc's 'Decision Summary' hard-picks the milestone shape ('the first wallet milestone IS Cashu/nutzap'), and the merge commit changed the nips.md notes column from 'Requires product/architecture decision before activation' → 'Requires activation work before any support claim,' erasing the deferral marker.

## Decision

De facto reversal is live on master: the deferral marker is gone and the milestone shape is hard-picked in the merged design doc. The session flagged the contradiction between the merged doc and #1001's deferral and asked the owner to either (a) record the reversal explicitly on #1001, or (b) reframe the design doc as conditional. The owner has not yet responded.

## Consequences

- Two durable documents now contradict each other: the merged design doc asserts a decided milestone shape while #1001 still records it as deferred
- The nips.md support matrix no longer carries the 'requires product/architecture decision' deferral marker for NIP-60/61
- If the owner confirms this is a deliberate reversal, #1001 needs an explicit recorded decision; if not, the design doc needs reframing as conditional and the nips.md note needs restoring
- A corrections PR was offered but not opened — blocked on the owner's call on finding A

## Open Tail

- Owner must decide: deliberate reversal (record on #1001) or hold the deferral (reframe doc + restore nips.md marker)
- Additional doctrine corrections identified but not yet applied: PaymentPort ownership silently reassigned from nmp-nip47 to nmp-wallet contradicting crate-boundaries §8; compat-alias migration violates standing 'no compat aliases — ever' rule; nmp-nwc codec crate omitted from ownership model

## Evidence

- transcript lines 194-201
- transcript lines 265-266
- transcript lines 283-285
- transcript lines 298-306

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-2-wallet-milestone-deferral-silently-reversed-by.json`](transcripts/2026-07-03-2-wallet-milestone-deferral-silently-reversed-by.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-2-wallet-milestone-deferral-silently-reversed-by.json`](transcripts/raw/2026-07-03-2-wallet-milestone-deferral-silently-reversed-by.json)
