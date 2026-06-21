---
type: episode-card
date: 2026-05-26
session: 95156e27-58fe-4e26-9530-1778033c4559
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/95156e27-58fe-4e26-9530-1778033c4559.jsonl
salience: architecture
status: active
subjects:
  - zap-protocol-boundary
  - lnurl-resolution
supersedes: []
related_claims: []
source_lines:
  - 1-3
  - 55-82
  - 1052-1076
  - 1078-1127
  - 1300-1387
  - 1654-1683
captured_at: 2026-06-18T05:50:28Z
---

# Episode: Zap action: shell sends intent, kernel resolves lnurl

## Prior State

ZapInput.lnurl was a required String; app shells had to provide lightning addresses explicitly; the 'Zap' palette action was a stub that showed 'use :zap <lightning-address> <sats> [comment] (requires :wallet connect first)' regardless of wallet state; close_palette() unconditionally reset mode to Normal; 'z' key was shown in help but not wired

## Trigger

User correction: 'chirp-tui shouldn't know about lnurl… an app should just say zap this event with this amount and with this message… scope concerns appropriately!' — plus the visible bug that palette Zap did nothing

## Decision

ZapInput.lnurl changed to Option<String> (serde default); FetchLnurlInvoiceCommand resolves lnurl from kernel profile cache via ctx.lnurl_for_pubkey() when None; TUI sends only {recipient_pubkey, amount_msats, target_event_id?, comment?} with zero protocol details; palette 'Zap' action and 'z' key open an input bar for 'sats [comment]'; close_palette() only resets mode when still in Palette (preserves InputBar)

## Consequences

- Shells are protocol-agnostic for zap — future NIP-61 support requires no app-layer changes
- Missing lnurl on a profile produces a clear toast: 'this user has no lightning address in their profile'
- Kernel::lnurl_for_pubkey() added as the single source-of-truth for lightning address resolution from cached kind:0 profiles
- pending_zap_pubkey and pending_zap_event_id fields added to AppState to carry context between palette selection and input bar dispatch

## Open Tail

- ADR-0026 Phase 2: bunker signers still cannot sign kind:9734 (zap requires local keys)
- V-43 future fix: kernel auto-pay instead of host-side pending_zap_pay flag

## Evidence

- transcript lines 1-3
- transcript lines 55-82
- transcript lines 1052-1076
- transcript lines 1078-1127
- transcript lines 1300-1387
- transcript lines 1654-1683

