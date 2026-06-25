# M15 — Native cross-platform: Android + Desktop

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — WoT + cross-platform + release (M12 Wallet deferred post-v1).

**Demo product:** Chirp and (where capabilities allow) podcast slice running on Android (Compose) and Desktop (iced), alongside the existing iOS shell. Cross-platform consistency test passes — same scripted scenario produces byte-identical `AppState` JSON on the v1 native platforms. Web/wasm moved post-v1 on 2026-06-11.

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

**Subsystem deliverables.**

- Cross-platform consistency test in `nmp-testing` — drives same scripted action sequence on iOS, Android, and desktop, snapshots `AppState` JSON at checkpoints, asserts byte-equal.
- Per-platform performance reports.

**Exit gate.**

- Twitter clone identical scripted scenario produces byte-identical `AppState` snapshots on iOS / Android / Desktop.
- All §7.16 performance budgets met on reference devices (iPhone 12, Pixel 6a, M1 mini).

**Runnable artifact.** Native-platform demo. Report in `docs/perf/m15/cross-platform.md`.

## Post-v1 web milestone

Moved out of v1 on 2026-06-11. The web port resumes after v1 under epic #2045 and owns:

- Browser runtime composition separation per ADR-0067 (nmp-browser-runtime owns composition; nmp-wasm is ABI glue).
- `nmp-wasm` production parity.
- OPFS-SQLite storage backend per ADR-0054 (gated behind ADR-0067).
- `nmp-nip07` browser-signer capability module.
- Web shell stack TBD (React + signals / Solid / Svelte — pick at start of milestone).
- Browser consistency coverage added to the native consistency harness.
- Incognito fallback to in-memory store with a visible warning.
