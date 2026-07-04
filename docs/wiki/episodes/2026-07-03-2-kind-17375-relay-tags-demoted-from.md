---
type: episode-card
date: 2026-07-03
session: 5ad70acc-1442-4343-92a7-f79b2fc59071
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5ad70acc-1442-4343-92a7-f79b2fc59071.jsonl
salience: product
status: active
subjects:
  - nip60-relay-tags
  - kind-17375
  - relay-selection-authority
supersedes: []
related_claims: []
source_lines:
  - 868-899
  - 903-935
  - 1516-1557
  - 1643-1648
captured_at: 2026-07-03T08:59:23Z
---

# Episode: kind:17375 relay tags demoted from authoritative to legacy hints

## Prior State

The kind:17375 wallet event's `relay` tags were treated as authoritative relay selection for wallet-related events. Doc comments in mint_announce.rs stated 'If the wallet's kind:17375 includes relay tags → use ONLY those relays.' The fields WalletConfig::relays and Nip60WalletHandle::relays() carried this authority, and the field was used directly in publish_nutzap_info to populate NutZapInfo.relays.

## Trigger

Issue #2865 required demoting legacy relay tags on kind:17375 from authoritative status. The design doc specifies that NIP-61 kind:10019 nutzap info events and NIP-65 fallback are the authoritative relay sources, owned by the future nmp-wallet crate — not the kind:17375 wallet config event.

## Decision

Renamed WalletConfig::relays → legacy_relay_hint and Nip60WalletHandle::relays → legacy_relay_hint across nip60_wallet.rs, wallet_event.rs, and nutzap_send.rs. Rewrote all doc comments in mint_announce.rs, nip60_wallet.rs, wallet_event.rs, and lib.rs that framed kind:17375 relay tags as authoritative. The NIP-61 NutZapInfo.relays field (in nutzap.rs) was left untouched as it is authoritative per the NIP-61 spec.

## Consequences

- No code path in nmp-nip60 treats kind:17375 relay tags as authoritative relay selection
- The field name legacy_relay_hint signals to all future consumers that these tags are non-authoritative metadata
- publish_nutzap_info now reads from legacy_relay_hint but the NIP-61 NutZapInfo.relays field retains its spec-mandated authority
- Future nmp-wallet crate owns the authoritative relay selection logic (kind:10019 + NIP-65 fallback)

## Open Tail

- The actual relay selection logic using kind:10019 + NIP-65 fallback will be implemented in nmp-wallet, which does not yet exist

## Evidence

- transcript lines 868-899
- transcript lines 903-935
- transcript lines 1516-1557
- transcript lines 1643-1648

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-2-kind-17375-relay-tags-demoted-from.json`](transcripts/2026-07-03-2-kind-17375-relay-tags-demoted-from.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-2-kind-17375-relay-tags-demoted-from.json`](transcripts/raw/2026-07-03-2-kind-17375-relay-tags-demoted-from.json)
