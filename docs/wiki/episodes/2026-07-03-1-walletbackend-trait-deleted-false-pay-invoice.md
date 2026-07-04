---
type: episode-card
date: 2026-07-03
session: 5ad70acc-1442-4343-92a7-f79b2fc59071
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5ad70acc-1442-4343-92a7-f79b2fc59071.jsonl
salience: architecture
status: active
subjects:
  - nmp-nip60
  - wallet-backend-trait
  - false-surface-removal
supersedes: []
related_claims: []
source_lines:
  - 180-192
  - 604-604
  - 803-833
  - 1469-1480
  - 1643-1648
captured_at: 2026-07-03T08:59:23Z
---

# Episode: WalletBackend trait deleted — false pay_invoice surface removed from nmp-nip60

## Prior State

nmp-nip60 contained a WalletBackend trait (backend.rs) that presented a unified wallet abstraction including pay_invoice, create_nutzap_proofs, and balance_sats. The pay_invoice method was a stub returning Err(Unsupported) with zero real callers — a runtime-failing action that advertised a capability the crate does not perform. The trait's doc comments framed it as the app-facing wallet seam.

## Trigger

Issue #2865 explicitly required removing or capability-gating false surfaces: 'an operation the backend cannot perform (e.g. pay_invoice stubs) must be an absent capability in Rust-owned state, not a runtime-failing action.' The design doc (nip60-nip61-wallet-design.md) places backend selection and the WalletBackend seam in the future nmp-wallet crate, not nmp-nip60.

## Decision

Deleted backend.rs entirely — the WalletBackend trait, its impl block on Nip60WalletHandle, and all references across lib.rs, nip60_wallet.rs, and nutzap_send.rs. The crate's description was rewritten to scope it as 'NIP mechanics only — backend selection, the wallet operation journal, and the WalletBackend seam live in nmp-wallet.' The crate now exposes only Cashu proof/crypto types, event codecs, and pure shape validation.

## Consequences

- nmp-nip60 no longer advertises a pay_invoice capability — the false surface is gone rather than runtime-failing
- The WalletBackend seam is reserved for nmp-wallet (not yet created); nmp-nip60 is now a pure NIP mechanics crate
- lib.rs module list and re-exports updated to remove backend module and its types
- All doc comments referencing WalletBackend as the app-facing abstraction were rewritten

## Open Tail

- nmp-wallet crate (the new home for WalletBackend) is not yet created — this session was explicitly told not to start it

## Evidence

- transcript lines 180-192
- transcript lines 604-604
- transcript lines 803-833
- transcript lines 1469-1480
- transcript lines 1643-1648

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-1-walletbackend-trait-deleted-false-pay-invoice.json`](transcripts/2026-07-03-1-walletbackend-trait-deleted-false-pay-invoice.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-1-walletbackend-trait-deleted-false-pay-invoice.json`](transcripts/raw/2026-07-03-1-walletbackend-trait-deleted-false-pay-invoice.json)
