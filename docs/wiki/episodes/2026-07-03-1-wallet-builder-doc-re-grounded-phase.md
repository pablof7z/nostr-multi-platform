---
type: episode-card
date: 2026-07-03
session: 1c293d33-5ec2-4689-b6c2-cd159d8b6bb7
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1c293d33-5ec2-4689-b6c2-cd159d8b6bb7.jsonl
salience: root-cause
status: active
subjects:
  - nip60-wallet-builder-doc
  - nmp-wallet-phase1
  - release-classification-2882
supersedes: []
related_claims: []
source_lines:
  - 207-225
  - 353-359
  - 396-402
captured_at: 2026-07-03T11:32:30Z
---

# Episode: Wallet builder doc re-grounded: Phase-1 code already landed

## Prior State

The premise given to the agent for drafting the NIP-60/61 builder doc (#2872) was that crates/nmp-wallet did not yet exist — the doc would be speculative. Separately, nmp-wallet was classified [[private_packages]] and nmp-nip60 was parked in the release manifest, blocking external consumers from git-rev pinning.

## Trigger

During research for the builder doc, the agent discovered Phase 1 of the wallet epic (#2864) had already landed on master: crates/nmp-wallet exists with real, tested Rust (action-name constants, WalletCapabilities, WalletProjection, operation-journal state machine, WalletBackend trait). A sibling worktree already held a near-complete high-quality draft of the same doc with accurate citations.

## Decision

Re-grounded the entire builder doc (docs/builder-guide/29-nip60-wallet.md, PR #2888) in the real codebase instead of speculation. The doc includes an honest 'live today vs pending' table: only nmp.wallet.connect/disconnect/pay_invoice (NWC) are dispatchable today via nmp-nip47; Cashu/nutzap action names and new types are real, tested, frozen-contract code but not yet wired to a live backend. Adopted the sibling draft rather than opening a competing PR.

## Consequences

- Builder doc accurately reflects current project state — external consumers (nutsack) can reference it for capability-gated UI patterns, fail-closed rules, and the single publish chokepoint.
- The 'live vs pending' distinction in the doc sets the doctrine for what builders can dispatch today versus what is frozen-contract-but-unwired.
- PR #2888 opened for review; nmp-nip60 was already un-parked in #2865 and is now a workspace member, making its release-classification as a public crate the logical next step.
- Release classification (#2882) identified as the top blocker for external consumers — dispatched as a separate background agent still in flight.

## Open Tail

- Release classification #2882 (flip nmp-wallet/nmp-nip60 to public crates in release/nmp-release.toml) is still under investigation by a background agent — no terminal decision yet on whether the flip gates on functional completeness or just packaging readiness.

## Evidence

- transcript lines 207-225
- transcript lines 353-359
- transcript lines 396-402

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-1-wallet-builder-doc-re-grounded-phase.json`](transcripts/2026-07-03-1-wallet-builder-doc-re-grounded-phase.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-1-wallet-builder-doc-re-grounded-phase.json`](transcripts/raw/2026-07-03-1-wallet-builder-doc-re-grounded-phase.json)
