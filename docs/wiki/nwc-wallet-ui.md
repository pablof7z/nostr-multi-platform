---
title: NWC Wallet UI — WalletView and ConnectWalletSheet
slug: nwc-wallet-ui
summary: The WalletView SwiftUI shows a disconnected state with a Connect Wallet button and a connected state displaying status badge, balance in sats, wallet npub, and
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
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
---

# NWC Wallet UI — WalletView and ConnectWalletSheet

## WalletView States

The WalletView SwiftUI shows a disconnected state with a Connect Wallet button and a connected state displaying status badge, balance in sats, wallet npub, and a Disconnect button. The Cashu indicator in WalletView is decorative only, as the M12 Cashu/NIP-60/NIP-61 crates do not exist yet. The pay-invoice sheet is currently a stub.

<!-- citations: [^274d6-11] [^57528-23] -->
## ConnectWalletSheet URI Input

The ConnectWalletSheet TextEditor disables autocapitalization and autocorrection to prevent iOS from capitalizing the nostr+walletconnect:// URI scheme. [^274d6-12]

## Connect Button Enablement

The Connect button enablement in ConnectWalletSheet uses a schemeLooksValid() helper that matches the Rust parser's case-insensitive scheme check. [^274d6-13]
## See Also

