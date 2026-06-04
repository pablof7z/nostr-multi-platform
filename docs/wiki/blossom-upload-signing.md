---
title: Blossom Upload Signing & Kernel Path
slug: blossom-upload-signing
summary: Blossom blob uploads must use the kernel's sign_with_account_nonblocking path rather than accepting raw secret_bytes, so that NIP-46 bunker users can authentica
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
  - session:83b5dae5-d3f4-4f4d-b12f-9d04d17c9139
---

# Blossom Upload Signing & Kernel Path

## Upload Signing

Blossom auth events must be signed through `nmp_app_sign_event_for_return` rather than in app Rust, since that capability shipped in v0.2.4. Avatar, agent-artwork, and shake-feedback uploads must be wired through `nmp_app_sign_event_for_return` immediately rather than left degraded waiting for a v0.2.5 dependency. A Blossom upload using a per-podcast NIP-F4 key requires registering the key once with `nmp_app_signin_nsec(make_active=0)` and passing that pubkey to `nmp_app_sign_event_for_return`. NMP owns key custody and signing (signer registry + sign-for-return); the app owns its own Blossom HTTP transport, signed via NMP. NMP must not absorb Blossom HTTP; that is per-app and not NMP's job. However, NMP should ship an idiomatic API for Blossom uploads rather than requiring apps to hand-roll continuation-scanning, base64 encoding, and header construction. Blossom v1 scope is upload only (BUD-02 PUT), with the namespace built to extend.

<!-- citations: [^f1b74-33] [^83b5d-1] [^83b5d-5] [^83b5d-10] -->
## See Also

