---
title: iOS Keychain Stale Secrets & Main-Actor Blocking
slug: ios-keychain-stale-secrets
summary: iOS keychain persists across app reinstalls and can contain stale Marmot secret keys from previous sessions that block the main actor if read synchronously duri
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-29
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:e8cb5967-4f11-488d-a27f-960e4c53f064
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# iOS Keychain Stale Secrets & Main-Actor Blocking

## Stale Keychain Secrets

iOS keychain persists across app reinstalls and can contain stale Marmot secret keys from previous sessions that block the main actor if read synchronously during apply(). [^fe79b-4]
KernelModel.apply() uses `cachedSecretKey` (in-memory, set only by signInNsec) instead of keychain retrieval to avoid blocking the main actor with synchronous I/O. [^fe79b-5]
On cold launch, auto-login restores the keychain secret when no NMP_TEST_NSEC env var is set, calling signInNsec and Marmot register with the stored secret. [^fe79b-223]
NMP_TEST_NSEC env var takes priority over keychain retrieval during auto-login to support UITest affordance. [^fe79b-225]
Removing the currently-active account clears cachedSecretKey and calls capabilities.deleteSecret(accountID:) so the next cold launch starts at onboarding. [^fe79b-228]
ChirpCapabilities provides a deleteSecret(accountID:) method for clearing keychain entries. [^fe79b-229]
The V-90 Site 2 (capability-worker seam) restore-read exemption is safe because iOS Keychain reads use kSecAttrAccessibleWhenUnlockedThisDeviceOnly with no biometric/LAContext path, and they run before the first UI frame. [^1903]

<!-- citations: [^fe79b-4] [^fe79b-5] [^fe79b-223] [^fe79b-225] [^fe79b-228] [^fe79b-229] [^e8cb5-1] [^4edd4-9] -->
## See Also

