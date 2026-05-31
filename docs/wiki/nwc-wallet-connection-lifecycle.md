---
title: NWC Wallet Connection Lifecycle — ActorCommands, Runtime, and Persistent Subscriptions
slug: nwc-wallet-connection-lifecycle
summary: WalletRuntime manages the NWC connection lifecycle
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:274d6f3c-5974-48a6-a985-570ae0ae805d
---

# NWC Wallet Connection Lifecycle — ActorCommands, Runtime, and Persistent Subscriptions

## Wallet Connection Lifecycle

WalletRuntime manages the NWC connection lifecycle. WalletConnection holds wallet_pubkey_hex, client_secret_hex, relay URLs, status, balance, and pending request tracking. WalletStatus in the kernel snapshot includes status (connecting/ready/error/disconnected), relay_url, wallet_npub, and balance_msats. [^274d6-4]


## Actor Commands

ActorCommand includes WalletConnect { uri }, WalletDisconnect, and WalletPayInvoice { bolt11, amount_msats: Option<u64> } variants. [^274d6-5]

## Relay Auth and Persistent Subscriptions

The kernel supports per-relay-role auth signers via a HashMap<RelayRole, RelayAuthCredentials>, allowing the wallet lane to authenticate with its NWC client keypair independently of the user's identity key. The compat methods bind_auth_signer and clear_auth_signer remain as wrappers delegating to the per-role set_relay_auth_signer and clear_relay_auth_signer for the Content role. The kernel supports persistent subscriptions via a HashSet<String> that prevents the EOSE auto-CLOSE logic from closing wallet REQ subscriptions. [^274d6-6]

## Connection and Disconnection Behavior

WalletConnect on connect registers the NWC client keypair as the auth signer for RelayRole::Wallet and registers the NWC subscription as persistent. WalletDisconnect clears both the auth signer and persistent sub registration for RelayRole::Wallet. NWC relay text frames are intercepted in the RelayEvent::Message handler before being passed to kernel.handle_message, so kind:23195 responses are decrypted by WalletRuntime. [^274d6-7]
## See Also

