---
type: episode-card
date: 2026-05-21
session: 156aa64b-42e1-4d3b-96ce-25b31fc06fec
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/156aa64b-42e1-4d3b-96ce-25b31fc06fec.jsonl
salience: product
status: active
subjects:
  - nip59-signer-for-seal
  - dm-seam-routing
  - remote-signer-adapter
supersedes: []
related_claims: []
source_lines:
  - 95-112
  - 493-500
  - 903-903
  - 1462-1475
captured_at: 2026-06-18T05:05:38Z
---

# Episode: Bunker NIP-44 adapter closes ADR-0026 Phase 2 — remote signers can now gift-wrap DMs

## Prior State

ADR-0026 Phase 1 landed `SignerForSeal` + `gift_wrap_with_signer` but `IdentityRuntime::active_signer_for_seal()` returned `None` for remote (NIP-46/NIP-07) accounts. Bunker users hit a toast + early-return when trying to send DMs — the seal path was inert for them.

## Trigger

Architectural audit (5 parallel agents) identified 'Problem 1: ADR-0026 DM Bunker Path Inert' — the `RemoteSignerHandle` references in `signer_seal.rs` were comment-only and `dm.rs` returned early for remote accounts. Advisor flagged a silent mid-chain timeout as load-bearing, requiring an explicit outer bound.

## Decision

Implemented `RemoteSignerForSeal` adapter in `nmp-core::commands::remote_signer_for_seal` that bridges `RemoteSignerHandle` → `SignerForSeal`, translating between `nostr::UnsignedEvent`/`Event` and substrate `UnsignedEvent`/`SignedEvent` types. Changed `IdentityRuntime.remote_signers` from `Box<dyn>` to `Arc<dyn>` so the adapter can share the handle. Added `GIFT_WRAP_TOTAL_TIMEOUT = 12s` in `nmp_nip59` as an outer bound on the remote chain. `dm.rs` now resolves a `SignerForSeal` for both local and remote accounts and hands it to `gift_wrap_with_signer`.

## Consequences

- Remote-signer (bunker) accounts can now send NIP-17 gift-wrapped DMs end-to-end — the Phase 2 gap is closed
- 4 new tests (3 unit + 1 actor-level E2E); 865 nmp-core --lib tests pass; doctrine-lint D0/D6/D7/D8/D9/D10/D11/D13/D15 clean
- PR #228 (dm-relay-fail-closed) conflicts with the new dm.rs — flagged for rebase
- `remote_signers` storage is `Arc<dyn>` but `AddRemoteSigner` command still takes `Box<dyn>` (boundary API unchanged, conversion on insertion)

## Open Tail

- NIP-57 zap gift-wrap should reuse the same `SignerForSeal` seam (not yet wired)
- NIP-65 kind:10050 publish path for bunker accounts still unaddressed (Problem 2 from audit)

## Evidence

- transcript lines 95-112
- transcript lines 493-500
- transcript lines 903-903
- transcript lines 1462-1475

