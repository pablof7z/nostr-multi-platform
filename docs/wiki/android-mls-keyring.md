---
title: Android MLS Keyring
slug: android-mls-keyring
topic: mls
summary: Android used an in-memory mock keyring in production, causing group secrets to be lost on every app restart.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# Android MLS Keyring

## Problem

Android used an in-memory mock keyring in production, causing group secrets to be lost on every app restart.

<!-- citations: [^78c8e-10] [^78c8e-32] -->
## KeystoreKeyringCapability

Android's KeystoreCredentialStore uses AES-256-GCM with a non-exportable lazily-generated key in AndroidKeyStore. Ciphertext+IV is stored base64-encoded in app-private SharedPreferences keyed by account_id. It must not use the deprecated Jetpack security-crypto EncryptedSharedPreferences. Keystore keyring capability handler dispatches use env.with_local_frame to reclaim JNI local refs on every call regardless of attach history.

<!-- citations: [^78c8e-11] [^78c8e-34] [^78c8e-92] -->
## Identity Restore

Android sign-in survives restart via the new identity restore path (nativeIdentityRestore using the capability-backed keyring). <!-- [^78c8e-12] -->

## Capability Architecture

The host keyring capability is the single secure-storage port (nmp.keyring.capability namespace with Store/Retrieve/Delete). Both Android and iOS route through this unified port rather than having per-platform special casing. The keyring-core CredentialStoreApi is replaced by a CapabilityCredentialStore backed by this host keyring capability on every platform. iOS uses KeychainCapability (the same port its nsec sign-in uses), unifying the secure-storage stack and allowing the deletion of the apple-native-keyring-store dependency with no compat shim, rather than per-platform cfg branches. iOS registerCapabilityHandler runs before restoreChirpIdentity on startup (KernelModel.swift:266 before :344), ensuring the keyring probe precedes any Marmot registration. Android gains a Keystore-backed keyring capability handler routing all capability namespaces through a synchronous JNI upcall, enabling MLS DB key persistence across app restarts and replacing the in-memory mock keyring that lost group secrets.

The capability request uses account_id format '{service}/{user}' (e.g. 'nmp.chirp.marmot/marmot-mls-db-key'). Both service and user must remain slash-free for the split_once round-trip to stay injective.

<!-- citations: [^78c8e-33] [^78c8e-58] [^78c8e-74] [^78c8e-93] -->
## Credential Store Initialization

Marmot credential store initialization probes the host keyring capability with a side-effect-free Retrieve. On success it uses the capability store; on failure or missing handler it falls back to an in-memory mock and reports keyring_unavailable=true. Only an explicit status:not_found from the keyring capability maps to NoEntry (allowing mdk to mint a fresh key); all other failures (transport failures, missing handlers, and undecodable envelopes) map to PlatformFailure, preventing silent re-keying of an existing MLS database. NMP_MARMOT_MOCK_KEYRING=1 remains as an escape hatch for headless CI/repl environments, unconditionally installing the in-memory mock store.

<!-- citations: [^78c8e-35] [^78c8e-59] [^78c8e-75] -->
## Migration Notes

Existing iOS installs hold the MLS key under apple-native-keyring-store coordinates. After the unified credential store PR, that key is unreachable and MarmotService::new returns a null handle. The remedy is delete+reinstall (no migration shim, per the no-compat-aliases rule).

<!-- citations: [^78c8e-36] [^78c8e-76] -->
## Identity Restore & E2E Verification

Android sign-in survives restart via the identity restore path (nativeIdentityRestore using the capability-backed keyring). The E2E acceptance test requires force-stopping both apps, relaunching, and verifying that a new message still decrypts after restart — this proves the Keystore/Keychain-backed MLS DB key persists (the original Android mock keyring lost secrets on restart). <!-- [^78c8e-37] -->
