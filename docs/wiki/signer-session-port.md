---
title: Signer-Session Port
slug: signer-session-port
topic: capability-socket
summary: The signer-session capability port (ADR-0050) generalizes the prior single-verb SignEventForAccount port into a signer-session capability covering sign, nip44_e
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
---

# Signer-Session Port

## Signer-Session Capability Port

The signer-session capability port (ADR-0050) generalizes the prior single-verb SignEventForAccount port into a signer-session capability covering sign, nip44_encrypt, and nip44_decrypt, with completions delivered as actor-mailbox messages instead of per-tick parked-receiver scans. ADR-0050 is fully landed on master across all five rungs: 6.1 ADR, 6.2a waking actor inbox, 6.2 three-verb port + unified park/drain, 6.3 gift-wrap chain + SignerForSeal deletion, 6.4 gift-unwrap through port + raw-Keys slot deletion, and 6.5 bounded bunker decrypt queue + decrypt_state projection. A shared sign_and_publish extraction (sign → publish → fan-out → snapshot → ActionAccepted) must be used by React/Note/Follow write actions to avoid triplication.

<!-- citations: [^2e544-33] [^2e544-68] [^bf035-170] -->
## Gift-Wrap & Unwrap Routing

Gift-wrap sealing chains through the port (encrypt then sign) replace the SignerForSeal driver-thread mechanism. Gift-unwrap routes through the Nip44DecryptForAccount port, deleting DmInboxProjection's raw-Keys slot. No per-envelope bunker unseal RPC is built; the signer-session port handles decrypt as a continuation chain instead. <!-- [^2e544-34] -->

## Decrypt Queue & Throttling

A bounded per-account decrypt queue (MAX_IN_FLIGHT_DECRYPTS=8) throttles remote-signer backfill and surfaces decrypt_state (ok|limited|unavailable) plus undecrypted_count as projection state. The delegated bulk-decrypt session capability (NIP-46 verb extension for bunker DMs) is deferred to open issue #1259, not implemented in K1.

<!-- citations: [^2e544-35] [^2e544-69] -->
## Pubkey-Only Accessor

A pubkey-only accessor (AppHost::active_pubkey) replaces the raw-keys slot for identity-only consumers, activating WOT bootstrap, DM relay-list, and self-zap receipts for bunker accounts. <!-- [^2e544-36] -->

## Bunker Lifecycle

Bunker correlation token threads through BunkerHookRequest and SignerReady, making make_active=0 honorable. Bunker cancel is detach (signal and drop) rather than join, and wait_for_first_open is cancellable, eliminating the 10-second actor freeze. <!-- [^2e544-37] -->

## 30-Day Priority & K1 Completion

The 30-day priority is K1 through gift-unwrap, K2 through global-hook slots, and the sync-soundness pair (un-floored NEG-OPEN + slot-lifetime cache-serve marker). K1 rungs 6.1 through 6.5 are all merged to origin/master, completing the signer-session port keystone. <!-- [^2e544-38] -->

## Signer-Session UI Rendering

The signer_state projection generalizes the former bunker_connection_state to cover both NIP-46 and NIP-55 backends with a signer_kind discriminant plus five is_* state flags. Android gallery and Chirp both have a SignerStateRow rendering both backends (signer relay / external signer), with awaiting-approval spinner and unavailable re-auth states.

<!-- citations: [^da6b1-61] [^da6b1-76] -->
