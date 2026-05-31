---
title: "NMP Marmot: MLS-over-Nostr Encrypted Groups"
slug: nmp-marmot
summary: nmp-marmot is a post-v1 milestone spec for Marmot (MLS-over-Nostr encrypted groups), wrapping marmot-protocol/mdk v0.7.1+.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:1cc3720b-c928-4fa6-8f39-9d995ae8ebc3
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:f8543716-09b7-4884-8952-da52f571962e
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# NMP Marmot: MLS-over-Nostr Encrypted Groups

## Overview

nmp-marmot is a v1 feature spec for Marmot (MLS-over-Nostr encrypted groups). The crate is named nmp-marmot (not nmp-mls) and wraps the Marmot Development Kit (mdk-core 0.8.0).
Marmot is MLS-over-Nostr, not NIP-29 (relay-based groups); the v0.2.0 changelog was corrected before release to avoid mislabeling the protocol.
Marmot coexists with NIP-17 rather than replacing it.
Marmot is classified as a non-NIP protocol module in the subsystem matrix rather than included in the NIP table.
nmp-app-chirp has a direct mdk-core dependency as a conscious FFI typed-translation-layer deviation from marmot-mls.md's literal 'sole importer' wording, but no MLS type crosses the C-ABI.
Android Chirp has no MLS/Marmot support: the build.gradle.kts never passes --features marmot, nmp-android-ffi uses default-features=false, and zero Marmot UI/FFI exists.
The Groups tab presents both NIP-29 (unencrypted, relay-managed) and MLS (Marmot, encrypted) sections, with the NIP-29 section always present regardless of MLS state.

<!-- citations: [^d27a4-10] [^7b4ae-3] [^1cc37-1] [^1cc37-2] [^1cc37-3] [^d27a4-9] [^42908-15] [^4edd4-23] -->
## Storage

MLS ratchet state uses mdk-sqlite-storage alongside NMP's LMDB. This is permissible since MLS epoch state is not Nostr event data.
Key-package fetch and cache logic (kp_cache) belongs in nmp-marmot's MarmotService (the shared protocol crate), not in nmp-app-chirp, so all NMP apps benefit.
Solo group creation (no invitees specified) proceeds without requiring key packages; the key_package_unavailable error fires only when invitees exist but no key packages are available.
V-61 surfaces an OrphanedCommit diagnostic in Marmot instead of silently dropping it. V-62 tracks that Marmot keyring failure surfaces a KeyringUnavailable diagnostic and installs an in-memory mock store, causing MLS secrets to be lost with a visible warning rather than silently.
The `marmot_db_dir()` function returns a stable platform-appropriate data path (`~/Library/Application Support/chirp-tui/marmot` on macOS, `~/.local/share/chirp-tui/marmot` on Linux) with PID-temp fallback.
The `credential_store.rs` cfg blocks include `target_os = "macos"` so that macOS uses the real Apple Keychain for Marmot SQLite encryption instead of a mock store.
A stable marmot data directory must be paired with the real OS keyring store initialization; using a stable path without the native keyring causes Marmot DB decryption failures on subsequent launches.
Headless/CI MLS testing on macOS is blocked because AppleStore::new() succeeds without entitlements, causing credential_store::initialize() to pick the real Keychain and short-circuit before the mock store, with no override available.
The NMP_MARMOT_MOCK_KEYRING opt-in escape hatch forces the in-memory mock store, enabling headless MLS testing; it is off by default and cannot activate in shipped apps.

<!-- citations: [^1cc37-4] [^d27a4-11] [^d27a4-12] [^cd2b6-13] [^f8543-4] [^42908-16] [^4edd4-21] -->
## Routing

Marmot uses the RelayPinned routing lane from M11.5 (ADR-0012) with no new compiler changes needed. The `nmp_marmot_snapshot` and `nmp_marmot_group_messages` symbols are live consumers that must be migrated to the push projection seam before their removal.
The Marmot messages projection uses two separate push projection keys (nmp.marmot.snapshot and nmp.marmot.messages keyed by group_id_hex) so that a new message does not re-emit the entire group list.
Projecting all groups' decrypted message tails is cheap because messages are already-decrypted plaintext in MDK SQLite (bounded newest-N rows), so view-state stays out of the kernel.
The KeyPackageLookupView ViewModule (namespace marmot.key_package_lookup) triggers kernel relay subscriptions for peer key packages via NIP-65 routing by declaring authors in its ViewDependencies.
The Marmot ingest tap registers for kinds [443, 444, 445, 1059, 30443] and fires `ingest_signed_event_core` for each incoming event.
Marmot's `publish_to` is currently a load-bearing internal consumer of `nmp_app_publish_signed_event_to` across crate boundaries via `extern "C"`; this dependency must be migrated to an internal kernel API `Kernel::publish_signed_explicit(event, relays)`, eliminating the FFI-across-crates pattern. PR-F scope includes `nmp_app_publish_unsigned_event` and coordinating Marmot's internal use of `publish_signed_event_to`, not just `nmp_app_publish_signed_event`.

<!-- citations: [^1cc37-5] [^d27a4-13] [^fe79b-10] [^1c093-18] [^d0690-3] [^4edd4-22] -->
## Welcome Messages

Marmot Welcome messages use NIP-59 gift-wrap. The milestone either follows post-v1 M9 or ships a standalone nmp-nip59 as Step 0.
Welcome delivery approximates to group relays rather than the recipient's NIP-65 inbox relays; proper inbox routing requires a future NIP-65 inbox resolver.

MarmotService caches Welcome IDs that failed processing and short-circuits retries to prevent repeated failed ingestion attempts. [^fe79b-11]

<!-- citations: [^1cc37-6] [^d27a4-14] -->
## See Also

