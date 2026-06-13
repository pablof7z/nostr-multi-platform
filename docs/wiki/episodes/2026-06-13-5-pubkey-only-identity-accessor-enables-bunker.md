---
type: episode-card
date: 2026-06-13
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: product
status: active
subjects:
  - active-pubkey
  - bunker-identity
  - app-host
supersedes: []
related_claims: []
source_lines:
  - 5034-5036
captured_at: 2026-06-13T19:22:03Z
---

# Episode: Pubkey-only identity accessor enables bunker account runtimes

## Prior State

Four identity-only consumers (WOT bootstrap, DM relay-list, self-zap receipts, NIP-51 mute list) gated on active_local_keys() which returned nothing for bunker/remote-signer-only accounts, making those runtimes dead for those users.

## Trigger

Architecture review Finding C — pubkey-only consumers dead for bunker accounts.

## Decision

Added AppHost::active_pubkey() backed by the existing kernel-populated ActiveAccountSlot (no second source of truth, D4). Migrated the four identity-only consumers to it. Left secret-key consumers (NIP-44 unseal, account-switch controller) on active_local_keys().

## Consequences

- Bunker/remote-signer-only accounts now activate WOT, DM-relay, self-zap-receipt, and mute-list runtimes
- Clear separation: pubkey-only consumers vs. secret-key consumers on different accessors
- No second source of truth — ActiveAccountSlot was already kernel-populated

## Open Tail

*(none)*

## Evidence

- transcript lines 5034-5036

