---
type: episode-card
date: 2026-05-25
session: 7b06d382-8fc2-4d52-bef5-f4d90e38cb2a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7b06d382-8fc2-4d52-bef5-f4d90e38cb2a.jsonl
salience: architecture
status: active
subjects:
  - nmp-nip57
  - nmp-nip47
  - zap-pay-chain
  - chirp-tui
supersedes: []
related_claims: []
source_lines:
  - 4944-5108
  - 5152-5648
captured_at: 2026-06-18T05:23:10Z
---

# Episode: V-43: backend owns full zap-pay chain — FetchLnurlInvoiceCommand auto-dispatches WalletPayInvoiceCommand

## Prior State

FetchLnurlInvoiceCommand emitted ShowToast with the bolt11 invoice string — the host app (iOS Chirp, chirp-tui) was expected to parse the toast and call wallet pay. Neither host actually did. The bolt11 died in ShowToast; zaps never completed. KernelBridge.swift line 368 had a stale comment claiming auto-dispatch (deleted V-41 behavior).

## Trigger

User asked: 'shouldn't the app commands just be 'zap' and that's it? the backend takes charge and zaps?' — confirming that the backend should own the entire LNURL→bolt11→NWC-pay flow and the host should never see the bolt11.

## Decision

FetchLnurlInvoiceCommand worker now auto-dispatches WalletPayInvoiceCommand after successfully fetching the bolt11, using nmp_nip47::active_wallet_runtime() (process-global slot, no NmpApp reference needed). nmp-nip57 added nmp-nip47 as an optional dependency under a feature flag. chirp-tui gained a :zap command that dispatches nmp.nip57.zap. If no wallet is installed, the worker emits RecordActionFailure with a descriptive reason instead of leaving the spinner hanging.

## Consequences

- nmp-nip57 → nmp-nip47 dependency edge exists (feature-gated), narrowing the D0 boundary: NIP crates may now reference each other's process-global runtime slots
- WalletPayInvoiceCommand.correlation_id set to the zap action's correlation_id (not None) so the action stage closes only when the NWC confirmation arrives, not when the invoice is fetched
- chirp-tui :zap <pubkey> <lnurl-or-address> <sats> [comment] is the only user-facing entry point — no wallet:pay boilerplate needed
- iOS KernelBridge.swift stale comment at line 368 still claims auto-dispatch — needs cleanup
- nmp.nip57.zap action is now end-to-end: LNURL fetch → bolt11 → NWC pay → success/failure notification

## Open Tail

- iOS Chirp still needs its zap button wired to nmp.nip57.zap (currently only TUI has :zap)
- KernelBridge.swift line 368 stale comment claiming WalletPayInvoice auto-dispatch

## Evidence

- transcript lines 4944-5108
- transcript lines 5152-5648

