---
type: episode-card
date: 2026-06-03
session: d8869714-0ee5-4fe3-94db-1efd068b1c58
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/d8869714-0ee5-4fe3-94db-1efd068b1c58.jsonl
salience: root-cause
status: active
subjects:
  - account-manager
  - verify-signer
  - add-unverified
supersedes: []
related_claims: []
source_lines:
  - 1308-1336
  - 1343-1408
  - 1597-1610
captured_at: 2026-06-11T22:59:26Z
---

# Episode: verify_signer probe and add_unverified removed

## Prior State

AccountManager.add() ran a sign-and-verify round-trip (verify_signer) on every account insertion — computing a canonical event id for a fixed kind:1 probe template, calling signer.sign(probe), and rejecting if the returned pubkey or event id didn't match. add_unverified() existed as an escape hatch for NIP-46 restore paths that couldn't sign eagerly. SignerMismatch and SignerError variants on AccountError existed solely for this probe.

## Trigger

Analysis showed the probe was unnecessary: local signers are deterministic crypto (can't misbehave), and NIP-46 bunkers are already authenticated by the handshake protocol. The probe added latency and add_unverified was a footgun that only existed because the probe blocked restore paths.

## Decision

Delete verify_signer, add_unverified, SignerMismatch, and SignerError. add() is now a plain idempotent insert keyed by hex pubkey (PD-004). AccountError reduced to just NotFound.

## Consequences

- Zero-latency account insertion — no sign-and-verify round-trip on every add()
- add_unverified() callers must use add() (same semantics now that probe is gone)
- AccountError::SignerMismatch and AccountError::SignerError deleted; any match arms on those variants will fail to compile
- Probe-only integration tests t3/t3b and their mutating-signer fixtures deleted; their one unique Arc::ptr_eq assertion folded into the surviving test
- Builder guide anti-patterns list updated: 'Skipping the add-time post-condition' entry removed

## Open Tail

*(none)*

## Evidence

- transcript lines 1308-1336
- transcript lines 1343-1408
- transcript lines 1597-1610

