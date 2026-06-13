---
type: episode-card
date: 2026-06-13
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: active
subjects:
  - signer-session-port
  - adr-0050
  - dm-inbox-projection
  - SignerForSeal
  - decrypt-state
supersedes:
  - 2026-06-13-1-signer-session-capability-port-replaces-signerforseal
related_claims: []
source_lines:
  - 5103-5107
  - 5246-5260
  - 5297-5310
  - 5463-5500
captured_at: 2026-06-13T19:44:01Z
---

# Episode: Signer-session capability port replaces ambient signer authority (K1/ADR-0050)

## Prior State

Signer interactions used ambient authority: the SignerForSeal execution model lashed a dedicated thread per signing account, and DmInboxProjection held an Arc<Mutex<Option<nostr::Keys>>> raw-Keys slot for inline decryption. Bunker (remote-signer) accounts could not decrypt (V-08 bug). remote_signer_unsupported was a boolean hiding state from the host.

## Trigger

Keystone K1 plan to port all signer operations through a capability-based interface; V-08 root-cause: raw-Keys slot makes bunker accounts structurally unable to decrypt because they lack local keys.

## Decision

ADR-0050 staged replacement across 5 rungs: (6.1) ADR spec; (6.2a) single waking actor inbox with mailbox completions; (6.2) three-verb capability port + unified park/drain + op_timeout; (6.3) gift-wrap chained through port, SignerForSeal execution model deleted; (6.4) gift-UNWRAP through port, raw-Keys slot deleted from DmInboxProjection; (6.5) bounded per-account decrypt queue with decrypt_state (ok/limited/unavailable) + undecrypted_count projection replacing remote_signer_unsupported:bool.

## Consequences

- Bunker accounts can now structurally decrypt — V-08 core fix shipped in Stage 4 (#1258)
- The decrypt bound self-targets only remote-signer accounts (local chains resolve inline), so no new kernel signer-kind slot was needed
- Host UIs (iOS/Android) now surface 'still decrypting' state instead of hiding pending messages
- Delegated decrypt-session capability (NIP-46 verb extension) deferred to open issue #1259
- SignerForSeal execution model fully deleted; only a thin trait/type shell remains
- FlatBuffers dm_inbox schema bumped to v2 for the new decrypt_state/undecrypted_count fields

## Open Tail

- #1259 (delegated decrypt-session NIP-46 verb) remains open — the ADR says it deserves its own ADR before implementation

## Evidence

- transcript lines 5103-5107
- transcript lines 5246-5260
- transcript lines 5297-5310
- transcript lines 5463-5500

