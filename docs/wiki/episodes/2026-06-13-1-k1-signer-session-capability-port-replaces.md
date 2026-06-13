---
type: episode-card
date: 2026-06-13
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - signer-session-port
  - adr-0050
  - signer-for-seal-deletion
  - bunker-decrypt
supersedes: []
related_claims: []
source_lines:
  - 4989-5019
  - 5098-5108
  - 5217-5286
  - 5354-5448
captured_at: 2026-06-13T18:45:45Z
---

# Episode: K1: Signer-session capability port replaces SignerForSeal

## Prior State

SignerForSeal was a thread-cluster that polled for signing completions; gift-wrap/unwrap went through separate seams; DmInboxProjection held an Arc<Mutex<Option<nostr::Keys>>> slot for decryption; bunker accounts could not structurally decrypt; identity-oracle temptation existed for distinguishing local vs remote signers.

## Trigger

Architecture review pattern P6 (bunker second-classness) — two independent reviewers converged on the same root cause: the signing surface was fragmented across SignerForSeal, raw-Keys slots, and polled completions, making bunker a second-class citizen.

## Decision

Replace the entire signing model with a signer-session capability port (sign | nip44_encrypt | nip44_decrypt), continuation-parked with mailbox completions delivered as actor messages. Five-rung ladder: 6.1 ADR-0050 spec → 6.2a single waking actor inbox → 6.2 three-verb port + unified park/drain + op_timeout → 6.3 gift-wrap chain through port (SignerForSeal deleted) → 6.4 gift-unwrap through port (raw-Keys slot deleted from DmInboxProjection, replaced by CommandSender + pubkey-only ActiveAccountSlot with epoch guard) → 6.5 bounded decrypt queue with decrypt_state surfacing. Rungs 6.1–6.4 landed on master; 6.5 written, patch saved, awaiting verification.

## Consequences

- SignerForSeal execution model fully deleted from codebase (6.3, #1255)
- DmInboxProjection no longer holds secret keys — decrypts via two-step port chain (nmp-nip17 inbox/chain.rs)
- Bunker accounts can now structurally decrypt DMs (V-08 fix delivered in 6.4, #1258)
- No new kernel signer-kind slot needed for 6.5 — the async-vs-inline resolution difference makes the bound self-target
- Capability variant deferred as issue #1259
- Three agent leads died (two on fable-5 model access loss, one on disk ENOSPC) — work survived via patch salvage

## Open Tail

- Rung 6.5 (bunker bulk-decrypt policy) written but unverified on disk crash; patch saved at /tmp/nmp-map-research/adr-0050-rung65.patch; needs rebase, cargo test, Swift/Android decoder updates for decrypt_state/undecrypted_count fields, then PR
- K2 (instance-scoped registration) and K3 (coverage ledger) remain gated behind K1 completion verification

## Evidence

- transcript lines 4989-5019
- transcript lines 5098-5108
- transcript lines 5217-5286
- transcript lines 5354-5448

