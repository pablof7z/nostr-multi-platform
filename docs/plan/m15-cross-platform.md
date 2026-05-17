# M15 — Cross-platform: Android + Desktop + Web

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — WoT + cross-platform + release (M12 Wallet deferred post-v1).

**Demo product:** Same Twitter slice and (where capabilities allow) podcast slice running on Android (Compose), Desktop (iced), and Web (wasm + React/Solid TBD). Cross-platform consistency test passes — same scripted scenario produces byte-identical `AppState` JSON on all four platforms.

**Scope.**

**Android port (~3 weeks):**

- Kotlin bindings via UniFFI; cargo-ndk + Gradle pipeline.
- Compose shell mirroring the iOS SwiftUI shell.
- `KeychainCapability` Android impl via `EncryptedSharedPreferences`.
- `nmp-nip55` Amber external-signer capability module.
- Android `FirebaseMessagingService` integration with `nmp-nip17-nse` for DM push (activates once M9 DMs land post-v1).

**Desktop port (~2 weeks):**

- iced shell (the development-time reference target lives on; this milestone graduates it to a shipping target).
- macOS + Linux + Windows.
- `KeychainCapability` impls per OS (macOS Keychain, Secret Service, Windows Credential Manager — already exists in `nostr-keyring`).

**Web port (~3 weeks):**

- `nmp-wasm` mature.
- IndexedDB storage backend; OPFS where supported.
- `nmp-nip07` browser-signer capability module.
- Web shell stack TBD (React + signals / Solid / Svelte — pick at start of milestone).

**Subsystem deliverables.**

- Cross-platform consistency test in `nmp-testing` — drives same scripted action sequence on all four targets, snapshots `AppState` JSON at checkpoints, asserts byte-equal.
- Per-platform performance reports.

**Exit gate.**

- Twitter clone identical scripted scenario produces byte-identical `AppState` snapshots on iOS / Android / Desktop / Web.
- All §7.16 performance budgets met on reference devices (iPhone 12, Pixel 6a, M1 mini, modern browsers).
- Web works in incognito mode by falling back to in-memory store with a visible warning.

**Runnable artifact.** Four-platform demo. Report in `docs/perf/m15/cross-platform.md`.
