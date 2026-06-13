---
type: episode-card
date: 2026-06-13
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - signer-session-port
  - signer-for-seal-deletion
  - dm-inbox-decrypt
  - bunker-second-classness
supersedes:
  - 2026-06-13-1-k1-signer-session-capability-port-replaces
related_claims: []
source_lines:
  - 4944-4950
  - 4989-4993
  - 5098-5107
  - 5271-5286
  - 5504-5514
captured_at: 2026-06-13T19:22:03Z
---

# Episode: Signer-session capability port replaces SignerForSeal thread cluster (ADR-0050 / K1)

## Prior State

Bunker/remote-signer accounts were structurally second-class: they couldn't decrypt DMs (V-08), SignerForSeal was a thread lash-up holding raw Keys, DmInboxProjection held Arc<Mutex<Option<nostr::Keys>>>, and gift wrap/unwrap went through old seams with no unified completion path.

## Trigger

Architecture review identified P6 (bunker second-classness) as a systemic pattern; two reviewers independently converged on a signer-session capability port as the root-cause fix.

## Decision

ADR-0050: replace SignerForSeal and the raw-Keys slot with a three-verb signer-session capability port (sign | nip44_encrypt | nip44_decrypt) using continuation-parked mailbox completions. Gift-wrap and gift-unwrap chain through the port. DmInboxProjection holds CommandSender + pubkey-only ActiveAccountSlot. Bounded per-account decrypt queue surfaces decrypt_state (ok|limited|unavailable) + undecrypted_count to hosts.

## Consequences

- SignerForSeal execution model fully deleted (Stage 3)
- Raw-Keys slot deleted from DmInboxProjection (Stage 4)
- Bunker accounts can now structurally decrypt DMs (V-08 core fix)
- DmInboxProjection no longer holds secrets — account-switch epoch guard prevents cross-account leaks
- Delegated decrypt-session capability deferred to issue #1259 (needs its own ADR)
- Five staged rungs (6.1–6.5) all landed on master: ADR → waking actor inbox → three-verb port → gift-unwrap through port → bounded decrypt queue

## Open Tail

- Issue #1259: delegated decrypt-session NIP-46 verb extension remains open
- Three residual SignerForSeal refs are thin trait/type remnants, not the thread lash-up

## Evidence

- transcript lines 4944-4950
- transcript lines 4989-4993
- transcript lines 5098-5107
- transcript lines 5271-5286
- transcript lines 5504-5514

