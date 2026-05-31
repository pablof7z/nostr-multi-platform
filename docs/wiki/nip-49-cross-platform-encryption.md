---
title: NIP-49 Cross-Platform At-Rest Encryption
slug: nip-49-cross-platform-encryption
summary: NIP-49 at-rest encryption is currently iOS Keychain only, leaving a gap for Android and desktop platforms.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-27
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:f8543716-09b7-4884-8952-da52f571962e
---

# NIP-49 Cross-Platform At-Rest Encryption

## Platform Scope

NIP-49 at-rest encryption is currently iOS Keychain only, leaving a gap for Android platforms. On desktop, chirp-tui provides a KeyringCapability backend implemented via the OS secret store (macOS Keychain / Linux Secret Service) using the `keyring` crate.

<!-- citations: [^57528-9] [^f8543-2] -->
## See Also

