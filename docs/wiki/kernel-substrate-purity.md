---
title: Kernel Substrate Purity (D0)
slug: kernel-substrate-purity
topic: kernel-boundary
summary: The kernel/FFI layer must remain a pure substrate with no NIP-specific or kind-specific knowledge, no UI debouncing, and no dead code
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-12
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:7f143c67-6e46-424a-90a8-5bf844947fee
  - session:b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Kernel Substrate Purity (D0)

## Kernel Substrate Purity

The NMP project (Nostr Multi-Platform) is a Rust framework for building Nostr apps that run natively on iOS, Android, Web (WASM), and desktop. All hard logic (relay connections, cryptography, subscriptions, event storage, signing) lives in a single Rust crate (nmp-core), and platform shells act as thin renderers. The kernel crate (nmp-core) is forbidden from naming any NIP protocol by name; all protocol-specific behavior plugs in through trait seams (substrate layer), a discipline called D0. The substrate trait seams are production-wired extension points with multiple real implementations across separate crates: IngestParser (3 impls), ActionModule (10+ impls), ProtocolCommand (6 impls), OutboxRouter/MailboxCache (injected at composition), and EventIngestDispatcher (multiplexed). The kernel/FFI layer must remain a pure substrate with no NIP-specific or kind-specific knowledge, no UI debouncing, and no dead code. Any construct that violates this boundary must be deleted or relocated to the appropriate layer.

UI debouncing is the host application's responsibility. The inflight_dispatches dedup guard (30s TTL on rapid re-taps) must not exist in the kernel/FFI layer. The creating_account_inflight guard in identity.rs is also a UI debounce that must be deleted from the kernel; hosts must debounce their own account-creation buttons.

Dead code must be removed. The nmp_app_refresh_replaceable FFI stub is never wired and its dispatch arm is a silent no-op; it must be deleted.

nmp-core currently has 12 D0 violations where NIP-specific and kind-specific knowledge leaked into the pure substrate kernel, documented in issue #920. All such leaks must be extracted to their proper crates.

TimelineItem is a D0 violation: it is a social-feed concept that must live in nmp-nip01, not in nmp-core. Its snapshot projection also fails to expose Nip10Refs (root ref, mentioned pubkeys), preventing iOS and Android from building full NIP-10 reply tags from available data. The pre-built reply_tags field (Vec<Vec<String>>) should be computed by nmp-nip01 and included in the snapshot projection so that shells forward it verbatim — zero NIP logic in the shell, zero NIP knowledge in nmp-core's struct definition.

The named C-ABI symbols (e.g. nmp_app_signin_nsec, nmp_app_add_relay, nmp_app_create_new_account) are the correct public API for framework-level operations (signin, account management, relay configuration, subscription lifecycle), not migration debt that should be eliminated. Forcing these operations through dispatch_action with JSON payloads would be slower (JSON roundtrip on security-sensitive paths), less safe (untyped string blob), and worse to document than a named C function signature. The FFI deprecation calendar and PD-039 backlog entry have been deleted, and plan.md exit criterion #7 rewritten to state the surface is frozen and enforced by a CI gate, with no migration-debt framing.

The nmp_app_* naming convention follows the C object-method pattern (library_type_verb), where 'app' denotes the NmpApp handle type, analogous to gtk_window_* or gtk_button_* in GTK. Operations like nmp_app_claim_profile and nmp_app_claim_event are framework-generic ref-counted subscription operations (not Chirp-specific, not user actions) that any app rendering a profile or event needs. Apps built on NMP should have their own C-ABI layer (e.g. podcast_app_* or nmp_app_chirp_*) that holds app-specific state and delegates Nostr operations down to nmp_app_*. The nmp_app_podcast FFI surface cannot re-export through libnmp_core.a because D0 forbids podcast-specific nouns from entering the core library; they land in their own archive.

<!-- citations: [^7f143-2] [^b4fe9-3] [^da6b1-53] [^da6b1-105] -->
