---
title: "NMP NWC Crate: NIP-47 Nostr Wallet Connect"
slug: nmp-nwc
summary: The `nmp-nwc` crate implements NIP-47 (Nostr Wallet Connect) as a standalone protocol crate with no dependency on `nmp-core`.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:274d6f3c-5974-48a6-a985-570ae0ae805d
  - session:50510273-d1c9-424a-b877-179d52fba557
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# NMP NWC Crate: NIP-47 Nostr Wallet Connect

## Overview

The `nmp-nwc` crate implements NIP-47 (Nostr Wallet Connect) as a standalone protocol crate with no dependency on `nmp-core`. V-38 (wallet out of nmp-core) is fixed: wallet is fully migrated to `crates/nmp-nip47/`.

The `nwc_probe` example binary exercises the full NWC protocol against real relays and can be used to diagnose wallet connectivity issues independently of the iOS app.

<!-- citations: [^274d6-1] [^274d6-2] [^42908-17] -->
## NWC URI Parsing

The `NwcUri` struct stores `wallet_pubkey_hex`, `client_secret_hex`, `relay_urls: Vec<String>`, and `lud16` fields. The URI parser (`NwcUri::parse`) accepts the `nostr+walletconnect://` scheme case-insensitively and normalizes it to lowercase before parsing to handle auto-capitalized input. It accepts multiple `relay=` query parameters, trims whitespace, deduplicates them, and uses the first relay URL as the primary relay. Unknown query parameters surface an error instead of being silently dropped (V-74).

<!-- citations: [^274d6-3] [^cd2b6-15] [^42908-18] -->
## Encryption

NWC uses NIP-04 encryption by default for kind:23194 requests and kind:23195 responses, with NIP-44 as a fallback. NIP-04 encrypted content is detected by the `?iv=` marker in the ciphertext; if absent, NIP-44 decryption is attempted as fallback. [^274d6-4]

## Client Identity and Event Kinds

The NWC client secret key is a separate dedicated keypair (not the user's Nostr identity) that signs kind:23194 requests and decrypts kind:23195 responses. Kind 23194 is the NWC client-to-wallet request event kind; kind 23195 is the wallet-to-client response event kind. [^274d6-5]

## NWC Methods

The `nmp-nwc` crate supports three NWC methods: `GetInfo`, `GetBalance`, and `PayInvoice`. The dead `MakeInvoice` API surface has been removed (V-77). The `PayInvoice` response is handled to address double-pay risk (previously the response was silently dropped), and a `correlation_id` is threaded through `WalletPayInvoice` to correlate requests with their responses. Pay_invoice success is detected when `error.is_none()`, not on preimage presence, making success detection robust. V-63/V-64 (NIP-47 payment correctness) uses an `on_idle_tick` seam that fires on every actor loop even when the NWC relay is silent, complying with D8 without polling.

<!-- citations: [^274d6-6] [^2c4ad-13] [^cd2b6-14] [^42908-19] [^4edd4-27] -->
## Wallet Relay Lane

The `RelayRole` enum includes a `Wallet` variant and a `Bunker` variant for the NIP-46 bunker relay seam. `RelayRole::Wallet` is a third relay transport lane excluded from `all()` so it does not block startup bootstrap gates or appear in standard relay statuses. Relay health entries for the Wallet lane are lazily initialized via `entry().or_default()`.

<!-- citations: [^274d6-7] [^50510-5] -->
## Per-Role Auth Signers

The kernel supports per-role auth signers via `auth_signers: HashMap<RelayRole, RelayAuthCredentials>`, allowing different relay lanes to authenticate with different keys. The old `bind_auth_signer`/`clear_auth_signer` methods are retained as compatibility wrappers that delegate to the per-role API. The kernel also supports persistent subscriptions via `persistent_subs: HashSet<String>`; EOSE auto-CLOSE skips subscriptions registered as persistent. [^274d6-8]

## Wallet Module Lifecycle

The wallet module registers the NWC client keypair as the auth signer for `RelayRole::Wallet` and registers the NWC REQ subscription as persistent upon connect. On wallet disconnect, the wallet module clears the per-role auth signer and unregisters the persistent sub for the Wallet relay lane. NWC relay text frames are intercepted before kernel ingestion; kind:23195 responses are decrypted by the wallet module, and AUTH challenges are handled by the kernel's per-role auth signer infrastructure (not by the wallet module directly). NIP-47 wallet connection uses the existing idle-tick seam for heartbeat and reconnect (V-79), and exposes a `connection_state` projection for the shell, so stale connections after an UNAUTHORIZED error or relay disconnect are detected and recovered rather than remaining silently broken.

<!-- citations: [^274d6-9] [^cd2b6-16] [^42908-20] -->
## WalletRuntime and State

The `WalletRuntime` manages actor-local NWC connection state (connection details, balance, pending requests) and is the sole writer of wallet state; the kernel snapshot is a read-only projection. WalletStatus in the kernel snapshot includes status (`connecting`|`ready`|`error`|`disconnected`), relay_url, wallet_npub, and balance_msats. [^274d6-10]

## Actor Commands

The `ActorCommand` enum includes `WalletConnect { uri: String }`, `WalletDisconnect`, and `WalletPayInvoice { bolt11: String, amount_msats: Option<u64> }` variants. [^274d6-11]

## FFI

FFI exposes `nmp_app_wallet_connect(app, uri)`, `nmp_app_wallet_disconnect(app)`, and `nmp_app_wallet_pay_invoice(app, bolt11, amount_msats_json)` as fire-and-forget C functions. [^274d6-12]

## iOS UI Considerations

The WalletView SwiftUI TextEditor disables autocapitalization and autocorrection to prevent iOS from altering the NWC URI. The Connect button in the ConnectWalletSheet enables only when the pasted URI starts with `nostr+walletconnect://` (case-insensitive via `schemeLooksValid()`). [^274d6-13]
## See Also

