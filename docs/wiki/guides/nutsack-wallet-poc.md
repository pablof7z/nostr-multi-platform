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
updated: 2026-07-04
verified: 2026-07-03
compiled-from: conversation
sources:
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:1c293d33-5ec2-4689-b6c2-cd159d8b6bb7
---

# Nutsack Wallet PoC and Test Harness

## Repository & Skeleton

The nutsack PoC repo lives at `/Users/pablofernandez/Work/nutsack`. It contains a Rust/TUI skeleton with NMP pinned by git rev and follows a thin-shell doctrine. nutsack is a genuine external consumer app of the nmp-wallet platform, confirmed via `gh issue view 2882`.

The TUI uses ratatui as the shell with a screen map and single-frame render smoke. The event loop is a documented TODO cloned from the in-repo nmp-gallery-tui. On first run — when `~/.config/nutsack/config.toml` is absent — the TUI launches an onboarding wizard; on every later run it skips straight to the wallet.

<!-- citations: [^91a86-9c032] [^91a86-a8227] [^1c293-5d502] [^91a86-aaa7d] -->
## nutsack-core

nutsack-core contains zero wallet logic. It provides typed `nmp.wallet.*` action builders, the bounded `WalletProjection` mirror, config (testnut + relays), and the `NutsackApp` composition handle over nmp-native-runtime. Real NMP deps are feature-gated off. Mint discovery in nutsack is curated rather than NIP-87-based (#2880 not implemented), and nutsack must not reimplement NIP-87 mint discovery.

<!-- citations: [^91a86-c1a53] [^91a86-0d014] -->
## Security Tripwire

nutsack includes a tripwire test asserting action payloads carry no proof, secret, or privkey. The real nsec at `~/.nsec-for-nutsack.txt` must never be logged, echoed, committed, or posted — it is passed only via the NUTSACK_NSEC env var.

<!-- citations: [^91a86-b5714] [^91a86-44c0c] -->
## Acceptance Test

The acceptance test uses an ephemeral relay via `nak serve`. The scenario: two fresh nsecs each create a wallet and publish nutzap info; each deposits value-less ecash from `testnut.cashu.space` (auto-settle, no Lightning); A nutzaps B and B nutzaps A; each redeems; and both balances/history are asserted via the projection only. <!-- [^91a86-f461f] -->

## Onboarding Wizard

The onboarding wizard offers a choice between importing an existing nsec and creating a new account.

The import-nsec screen uses masked input and validates the nsec via `nostr::SecretKey::from_bech32`, showing an inline error on invalid input and allowing retry.

The create-account screen generates a keypair via `nostr::Keys::generate()`, shows npub + nsec with a red 'WRITE THIS DOWN — it's the only copy' warning, and lets Enter confirm or Esc go back to re-roll.

The relay checklist pre-checks 5 curated relays (damus.io, nos.lol, primal.net, nostr.band, purplepag.es) with space to toggle and `a` to add a custom relay, requiring at least 1 selected.

The mint checklist pre-checks 4 curated real mints (minibits, coinos, 0xchat, lnvoltz) plus testnut, using the same checklist UX as the relay screen. <!-- [^91a86-677d1] -->

## Configuration & Persistence

Nutsack persists configuration to `~/.config/nutsack/config.toml` (or `$XDG_CONFIG_HOME/nutsack/config.toml`) as TOML with mode 0600, containing nsec, relays, and mints.

The `NUTSACK_NSEC` env var bypasses onboarding and the config file entirely as a dev/power-user override, is never persisted, and is never echoed back. <!-- [^91a86-f5516] -->

## Wallet View

The wallet view has 6 tabs: Home, Deposit, Send, Receive, Feed, and Settings.

The Settings tab shows the session pubkey only (nsec never shown after the backup screen) and displays 3 columns: published kind:10019 contents, configured relays, and configured mints.

The Feed tab top pane shows received nutzaps (verified/rejected badge, amount, mint, event id) and the bottom pane shows history filtered to nutzap_send/nutzap_redeem only (not deposits/pay_bolt11). Empty-state-honest messages are shown when there are no rows to display (e.g., 'no received nutzaps yet — sender pubkey/timestamp aren't in the projection yet'). <!-- [^91a86-40303] -->

## Projection Gaps & Display Limitations

Nutzap-send history rows carry `amount: 0` in the projection — a display-field-population gap on NMP's side where the balance itself is correct.

ReceiveCandidate rows lack sender pubkey and timestamp in the projection, rendered as absent rather than reconstructed from raw events.

Nutsack does not implement wallet recovery — importing an nsec that already has a NIP-60 wallet orphans it because `cashu.recover` was unconditionally rejected (no backend implemented it).

The TUI should be able to display which mints were used (from/to) and any fees paid for a nutzap, but should not contain wallet-logic for cross-mint transfers. <!-- [^91a86-af1c2] -->

## Real-Sats Runner

The real-sats runner binary is `real-sats-nutzap.rs` in nutsack-tui, wired via an explicit `[[bin]]` with `required-features = ["nmp-backend"]`, and is skipped by the default no-feature build.

The runner drives the sequence: select_backend → cashu.create → nutzap.publish_info → cashu.deposit_quote (prints bolt11, pauses/polls) → cashu.complete_deposit → nutzap.send.

It takes NUTSACK_NSEC as the only secret input (never logged, never file-read by the binary), with optional overrides MINT_URL, RELAYS, RECIPIENT_NPUB, and AMOUNT_SATS (default 21).

Default relays are exactly damus.io, nos.lol, primal.net, and purplepag.es (not nostr.band, which is unreachable from some networks).

Default MINT_URL is https://mint.minibits.cash/Bitcoin and default RECIPIENT_NPUB is the owner's npub (npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft).

The runner polls for deposit payment every 15 seconds, backing off 45 seconds on HTTP 429 or rate-limit responses, and surfaces the mint quote_id for recoverability.

NUTSACK_RECOVER=1 mode calls cashu.recover + poll total_balance, then sends AMOUNT_SATS; otherwise it does create+deposit and sends balance.saturating_sub(5) (SEND_FEE_BUFFER). <!-- [^91a86-45986] -->

## Write-Ahead Log

Nutsack uses a durable WAL at `~/.nutsack-wal` so the money-safe saga can reconcile stalled operations on restart. <!-- [^91a86-32191] -->
