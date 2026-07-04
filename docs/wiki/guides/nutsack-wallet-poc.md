---
title: Nutsack Wallet PoC and Test Harness
slug: nutsack-wallet-poc
topic: wallet-architecture
summary: The nutsack PoC repo lives at `/Users/pablofernandez/Work/nutsack`
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-03
updated: 2026-07-03
verified: 2026-07-03
compiled-from: conversation
sources:
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:1c293d33-5ec2-4689-b6c2-cd159d8b6bb7
---

# Nutsack Wallet PoC and Test Harness

## Repository & Skeleton

The nutsack PoC repo lives at `/Users/pablofernandez/Work/nutsack`. It contains a Rust/TUI skeleton with NMP pinned by git rev and follows a thin-shell doctrine. nutsack is a genuine external consumer app of the nmp-wallet platform, confirmed via `gh issue view 2882`.

The TUI uses ratatui as the shell with a screen map and single-frame render smoke. The event loop is a documented TODO cloned from the in-repo nmp-gallery-tui.

<!-- citations: [^91a86-9c032] [^91a86-a8227] [^1c293-5d502] -->
## nutsack-core

nutsack-core contains zero wallet logic. It provides typed `nmp.wallet.*` action builders, the bounded `WalletProjection` mirror, config (testnut + relays), and the `NutsackApp` composition handle over nmp-native-runtime. Real NMP deps are feature-gated off. <!-- [^91a86-c1a53] -->

## Security Tripwire

nutsack includes a tripwire test asserting action payloads carry no proof, secret, or privkey. <!-- [^91a86-b5714] -->

## Acceptance Test

The acceptance test uses an ephemeral relay via `nak serve`. The scenario: two fresh nsecs each create a wallet and publish nutzap info; each deposits value-less ecash from `testnut.cashu.space` (auto-settle, no Lightning); A nutzaps B and B nutzaps A; each redeems; and both balances/history are asserted via the projection only. <!-- [^91a86-f461f] -->
