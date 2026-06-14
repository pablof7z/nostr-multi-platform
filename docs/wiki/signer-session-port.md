---
title: Signer-Session Port
slug: signer-session-port
topic: capability-socket
summary: The signer-session capability port (ADR-0050) generalizes the prior single-verb SignEventForAccount into a backend-transparent signer-session capability coverin
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

The signer-session capability port (ADR-0050) generalizes the prior single-verb SignEventForAccount into a backend-transparent signer-session capability covering sign, nip44_encrypt, and nip44_decrypt, continuation-parked through the existing PendingSignReturn machinery with completions delivered as actor-mailbox messages. Generalizing the signing port collapses SignerForSeal, its driver threads, and the per-crate raw-Keys slots into one mechanism. The signer-session port (K1) discharges P6's root cause and is the seam V-08's fix currently has nowhere to land on. ADR-0050 is fully landed on master across all five rungs: 6.1 ADR (PR #1198), 6.2a waking actor inbox (PR #1221), 6.2 three-verb port + unified park/drain (PR #1242), 6.3 gift-wrap chain + SignerForSeal deletion (PR #1255), 6.4 gift-unwrap through port + raw-Keys slot deletion (PR #1258), and 6.5 bounded bunker decrypt queue + decrypt_state projection (PR #1322). PendingSign publish drain is unified with the sign-port's PendingSignReturn sink machinery, deleting the ~90-line inline duplicate. A shared sign_and_publish extraction (sign → publish → fan-out → snapshot → ActionAccepted) must be used by React/Note/Follow write actions to avoid triplication. (Previously: the signing port was cut at the wrong altitude — 'sign one event' instead of a signer-session capability — causing three parallel mechanisms.)

<!-- citations: [^2e544-33] [^2e544-68] [^bf035-170] [^2e544-354] [^2e544-379] [^2e544-401] [^2e544-418] [^2e544-472] -->
## Gift-Wrap & Unwrap Routing

Gift-wrap is a continuation chain through the signer-session port (encrypt→sign), replacing SignerForSeal, its driver threads, and both timeout constants, which are deleted. Gift-unwrap routes through the Nip44DecryptForAccount signer port; the raw-Keys slot is deleted from DmInboxProjection, enabling bunker accounts to decrypt DMs. No per-envelope bunker unseal RPC is built; the signer-session port handles decrypt as a continuation chain.

<!-- citations: [^2e544-34] [^2e544-355] [^2e544-419] [^2e544-457] [^2e544-474] -->
## Decrypt Queue & Throttling

Bunker accounts have a bounded per-account decrypt queue (MAX_IN_FLIGHT_DECRYPTS=8) that throttles remote-signer backfill and surfaces decrypt_state (ok/limited/unavailable) plus undecrypted_count as projection state, never silently dropped. The delegated decrypt-session NIP-46 verb extension for bunker DMs is deferred to issue #1259; it deserves its own ADR before implementation and is not implemented in K1.

<!-- citations: [^2e544-35] [^2e544-69] [^2e544-421] [^2e544-475] -->
## Pubkey-Only Accessor

A pubkey-only active_pubkey() accessor on AppHost allows bunker/remote-signer accounts to activate WOT bootstrap, DM-relay-list, self-zap receipts, and NIP-51 mute list runtimes without accessing secret-key material, replacing the previous active_local_keys() accessor that caused pubkey-only consumers to hold full secret material and be silently dead for bunker accounts. (Previously: WOT bootstrap and self-zap-receipt interests were silently dead for bunker accounts because their consumers held the raw-keys slot, which was None for remote signers.)

<!-- citations: [^2e544-36] [^2e544-356] [^2e544-380] [^2e544-402] [^2e544-420] [^2e544-440] [^2e544-473] -->
## Bunker Lifecycle

Bunker correlation token threads through BunkerHookRequest and SignerReady, making make_active=0 honorable. Bunker cancel is detach (signal and drop) rather than join, and wait_for_first_open is cancellable, eliminating the 10-second actor freeze. <!-- [^2e544-37] -->

## 30-Day Priority & K1 Completion

The 30-day priority is K1 through gift-unwrap, K2 through global-hook slots, and the sync-soundness pair (un-floored NEG-OPEN + slot-lifetime cache-serve marker). K1 rungs 6.1 through 6.5 are all merged to origin/master, completing the signer-session port keystone. <!-- [^2e544-38] -->

## Signer-Session UI Rendering

The signer_state projection generalizes the former bunker_connection_state to cover both NIP-46 and NIP-55 backends with a signer_kind discriminant plus five is_* state flags. Android gallery and Chirp both have a SignerStateRow rendering both backends (signer relay / external signer), with awaiting-approval spinner and unavailable re-auth states.

<!-- citations: [^da6b1-61] [^da6b1-76] -->
