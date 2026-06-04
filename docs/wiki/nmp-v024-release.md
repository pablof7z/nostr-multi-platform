---
title: NMP v0.2.4 Release
slug: nmp-v024-release
summary: The nmp_v0.2.4 release includes nmp_app_sign_event_for_return and the make_active parameter addition to nmp_app_signin_nsec, nmp_app_signin_bunker, and nmp_app_
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-04
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
  - session:83b5dae5-d3f4-4f4d-b12f-9d04d17c9139
---

# NMP v0.2.4 Release

## nmp_v0.2.4 Release

The nmp_v0.2.4 release includes nmp_app_sign_event_for_return and the make_active parameter addition to nmp_app_signin_nsec, nmp_app_signin_bunker, and nmp_app_create_new_account. The nmp_app_sign_event_for_return FFI function provides synchronous-sign-and-return for external consumers (Blossom auth, ShakeFeedback) by returning a correlation_id and surfacing the signed event via a signed_events projection; it signs kind:24242 Blossom auth events for both active and non-active custodied keys, selecting a key via account_pubkey_hex, where an empty string selects the active account and a non-empty string selects a named custodied signer. The nmp_app_signin_nsec, nmp_app_signin_bunker, and nmp_app_create_new_account FFI functions accept a make_active parameter (1=activate, 0=register without activating) to support non-active signer registration for agent/secondary keys. A separate nmp_app_add_signer_nsec FFI symbol must not be introduced; non-active signer registration goes through nmp_app_signin_nsec with make_active=0. Starting in v0.2.4, nmp_app_sign_event_for_return provides sign-for-return capability, enabling signing Blossom auth events (kind:24242) and agent/secondary keys without the active account, and returning the signed JSON via the signed_events projection.

<!-- citations: [^f1b74-32] [^f1b74-41] [^83b5d-3] [^83b5d-8] [^83b5d-21] [^83b5d-29] -->
## nmp_v0.2.5 Release

NMP v0.2.5 is cut with PublishRaw signer_pubkey, the SignEventForAccount port, and nmp-blossom. [^83b5d-22]

The V-78 nmp-nip57 reconcile is implemented, migrating zap signing onto the unified SignEventForAccount port and landing on NMP master. [^83b5d-30]
## See Also

