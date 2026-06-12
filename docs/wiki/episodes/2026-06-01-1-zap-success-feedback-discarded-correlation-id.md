---
type: episode-card
date: 2026-06-01
session: 89070aba-0e77-4da3-99e1-322addb1c747
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/89070aba-0e77-4da3-99e1-322addb1c747.jsonl
salience: reversal
status: active
subjects:
  - nip57-zap-feedback
  - action-lifecycle
  - chirp-swift
supersedes: []
related_claims: []
source_lines:
  - 83-101
  - 103-103
  - 699-728
  - 916-951
  - 1225-1240
captured_at: 2026-06-11T22:52:16Z
---

# Episode: Zap success feedback: discarded correlation ID was the only gap

## Prior State

Pending decision PD-037/PD-034 assumed that kind:9735 zap receipts required substantial new infrastructure — adding nmp-nip57 dep, building a ZapsView projection, registering a domain, and decoding in Swift. The initial agent brief was marked as stale/wrong, and the recommendation was to defer until a zap ADR lands.

## Trigger

User rejected the defer recommendation and demanded a code-grounded proposal: 'Give me an actual proposal of how to implement properly, actually read code, not just use old docs.'

## Decision

Code investigation revealed the zap pipeline is ~90% complete: ZapsAggregateProjection is already registered, Swift decodes it, timeline zap counts update live, and NWC payment flow records Accepted/Failed correctly. The only gap is Swift discarding the zap correlation ID (@discardableResult on KernelModel.zap()), so the user sees no feedback after payment. Fix: capture the correlation ID, observe actionLifecycle via .onChange, and render a success/error toast — the identical pattern RelaySettingsView already uses for DM-inbox publish.

## Consequences

- Zap success feedback (haptic + toast) now follows the established action-lifecycle pattern rather than requiring new infrastructure
- KernelModel.zap() no longer marked @discardableResult — callers must handle the DispatchResult
- lastSuccessToast added alongside lastErrorToast in KernelModel, establishing a dual-toast pattern in RootShell
- Stale PD-037/PD-034 entries in the audit log are now resolved; no ADR or multi-PR effort is needed

## Open Tail

- ZapsAggregateProjection currently shows aggregate zap counts on timeline rows but has no detail-screen consumer yet (deferred, not blocked)
- Build failure on KernelTypes.generated.swift (RenderIdentifiable) pre-existed and is unrelated

## Evidence

- transcript lines 83-101
- transcript lines 103-103
- transcript lines 699-728
- transcript lines 916-951
- transcript lines 1225-1240

