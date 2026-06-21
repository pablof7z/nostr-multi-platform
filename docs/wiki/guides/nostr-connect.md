---
title: Nostr Connect URI
slug: nostr-connect
topic: marmot
summary: "The nostrConnectURI call in KernelBridge.swift:128 requires a default relay argument"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-06-19
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:27a9cbf3-1348-44f6-bc0f-95a0a9c6ad84
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:019edc05-2b24-72d3-88aa-2db67fdc57b5
  - session:019edc59-7035-7ba3-95cc-789d362adff2
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edd2a-7995-7123-8e7e-56f3d9acbb60
---

# Nostr Connect URI

## Kernel Bridge

The nostrConnectURI call in KernelBridge.swift:128 requires a default relay argument. NmpDefaults.nostrconnect_bootstrap_relay defaults to None; NMP ships no relay URL for nostrconnect, and leaf apps that want a fallback must supply Some(url) explicitly. The nostrconnect bootstrap relay URL must be removed from NmpDefaults::default() and instead required as explicit config via NostrConnectBootstrap::Relay(url) or NostrConnectBootstrap::Disabled, so starting a nostrconnect flow while unset fails observably rather than choosing a relay. The hardcoded NOSTRCONNECT_DEFAULT_RELAY_URL of wss://relay.damus.io in nmp-core constitutes a D0 violation and third-party dependency.

Making nostrconnect_bootstrap_relay None-by-default does not break any path that assumed a relay was always wired; the FFI returns null/fails closed when no explicit relay exists.

The nostrconnect permission set is app-supplied; NMP supplies no default, and when None the URI omits the &perms= parameter entirely rather than including an empty one. When a permission string is provided as Some, the value is percent-encoded as a whole (e.g. sign_event:1,sign_event:7 → sign_event%3A1%2Csign_event%3A7) and appended as &perms=<encoded> after the relay, secret, and name query parameters. The nostrconnect_perms slot is Arc<Mutex<Option<String>>>, conforming to D14 (which bans Arc<Mutex<Vec>>). NmpDefaults.nostrconnect_perms defaults to None and is wired only when Some, without disturbing the existing bootstrap-relay wiring. The FFI reads nostrconnect_perms as Option<String> and passes it through; lock failure degrades to None with no panic path. All callers of start_nostrconnect_handshake are updated to the new 2-arg signature (relay_url, perms: Option<String>).

<!-- citations: [^019ed-38] [^27a9c-6] [^fd809-2] [^cd2b6-9] [^019ed-37] [^019ed-94] [^11850-211] [^019ed-146] -->
## Login-Block Detection

The login-block component detects Amber (`nostrsigner`), Primal (`primal`), and generic NIP-46 bridges (`nostrconnect`). Olas has no URL scheme and cannot be detected via `UIApplication.shared.canOpenURL`. <!-- [^45258-16] -->

## NWC URI Parser

The NWC URI parser silently drops unknown parameters, including misspelled `relay=` entries. <!-- [^cd2b6-10] -->
