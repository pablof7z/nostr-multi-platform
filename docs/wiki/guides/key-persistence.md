---
title: Key Persistence
slug: key-persistence
topic: data-persistence
summary: Marmot registration auto-restores on cold relaunch by persisting the nsec secret to the iOS Keychain and re-registering when the first kernel snapshot arrives w
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-27
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:27a9cbf3-1348-44f6-bc0f-95a0a9c6ad84
  - session:3ed0a030-6daf-4680-9172-992f98deb328
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:c4b2e655-ca6b-42d2-9383-89bf52215d0a
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# Key Persistence

## Key Persistence

Marmot registration auto-restores on cold relaunch by persisting the nsec secret to the iOS Keychain and re-registering when the first kernel snapshot arrives with an active local account. Key package publishing must work automatically after account creation. The Rust FFI layer initializes the Apple keyring store via keyring_core::set_default_store before MarmotService::new, and falls back to a mock in-memory store when AppleStore::new fails (e.g. on simulator lacking entitlements); this fallback silently installs an in-memory mock that causes MLS secrets to be lost without warning (V-62). If MarmotService::new fails due to a stale unencrypted database, the FFI deletes the DB, WAL, and SHM files and retries once. The active_local_nsec Arc<Mutex<Option<String>>> slot in NmpApp is written synchronously by the actor BEFORE emitting the identity-change snapshot, guaranteeing race-free reads by the time Swift's apply() runs on MainActor. The nmp_app_chirp_marmot_register_active function reads the nsec from the NmpApp active_local_nsec slot rather than receiving it from Swift, so createAccount never needs to expose the secret key to the Swift layer. In KernelModel.apply(), when signerKind is 'local' but cachedSecretKey is nil (the createAccount path), Marmot registration calls registerActive() which reads the nsec from the Rust slot instead of requiring Swift to provide it. IdentityRuntime.active_nsec_bech32() returns the bech32 secret key of the currently active identity, used to populate the shared slot for Marmot auto-registration. The iOS keychain persists across reinstalls (scoped to bundle ID + access group, not app container), causing stale Marmot secrets to block main actor when retrieved via capabilities.retrieveSecret.

<!-- citations: [^27a9c-4] [^3ed0a-1] [^fe79b-2] [^c4b2e-3] [^cd2b6-5] -->
