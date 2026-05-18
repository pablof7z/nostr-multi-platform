# NmpPulse — e2e validation iOS app for NMP kernel

**Status (T66 / pulse-builder):** **L1 scaffold landed.** Timeline-only.
Sign-in / Compose / multi-account / NoteDetail screens are filed as T66a
because they depend on FFI surface that the kernel actor does not yet
expose (sign-in commands, publish-engine wiring, AccountManager
integration). See `Views/PendingFeaturesView.swift` for the in-app status
surface.

For the full spec see `docs/builder-guide/e2e-validation-app.md` and the
build guide `docs/builder-guide/e2e-validation-build.md`.

---

## What's wired

- **Bridge layer.** `Bridge/NmpCore.h` + `Bridge/KernelBridge.swift` —
  Path-A raw C FFI to the `nmp_core` static library. Same shape as
  `ios/NmpStress`, smaller surface (Pulse only consumes the timeline-
  reading subset).
- **`KernelModel`.** `@MainActor`-isolated `ObservableObject` decoding the
  JSON snapshot the actor pushes via `nmp_app_set_update_callback`.
- **TimelineView.** Live kind:1 feed from the kernel-bootstrap pubkey
  (`crates/nmp-core/src/relay.rs::TEST_PUBKEY`).
- **DiagnosticsView.** D6+D8 observability — rev counter, snapshot count,
  per-relay status (connection, auth, active wire subs).
- **PendingFeaturesView ("More" tab).** Honest in-app surface listing
  scope-deferred features + the substrate pieces that did land
  (Nip65OutboxResolver, ActiveAccountReactor, real-relay smoke test).

## Build for simulator

```bash
# 1. Build the iOS-sim static lib (Apple Silicon Mac).
cargo build -p nmp-core --target aarch64-apple-ios-sim

# 2. Generate the Xcode project (whenever project.yml changes).
cd ios/NmpPulse && xcodegen generate

# 3. Build the app.
xcodebuild -project NmpPulse.xcodeproj \
  -scheme NmpPulse \
  -sdk iphonesimulator \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath ./build \
  build

# 4. Install + launch.
APP_PATH="./build/Build/Products/Debug-iphonesimulator/NmpPulse.app"
xcrun simctl install booted "$APP_PATH"
xcrun simctl launch booted com.example.NmpPulse

# 5. (Optional) Screenshot.
xcrun simctl io booted screenshot /tmp/pulse.png
```

Verified PASS on Apple Silicon Mac + iPhone 17 simulator (iOS 26.5), May
2026: app launches, Timeline tab populates with live notes from the
bootstrap pubkey within a few seconds, rev counter increments,
DiagnosticsView shows connecting / connected transitions per relay.

## Install on physical iPhone

See `INSTALL-ON-IPHONE.md` in this directory.

## Demo walkthrough (what works today)

1. **Cold launch.** Tap `NmpPulse.app`. Expect: Timeline tab visible
   within 2s, "Waiting for kernel snapshot…" placeholder briefly, then
   timeline starts populating.
2. **Timeline.** Scroll the feed. Pull-to-refresh is not wired yet;
   updates arrive automatically as the kernel emits.
3. **Diagnostics.** Tap the gauge tab. Observe relay status table with
   `wss://relay.primal.net` and `wss://purplepag.es` (the kernel's
   bootstrap relays — not Pulse-specific). Rev counter and snapshot
   counter increment in real time.
4. **More.** Status surface showing what's deferred (T66a) vs what's
   substrate-complete.

## What this PR did NOT ship (filed as T66a)

| Screen | Blocker |
|---|---|
| Onboarding (paste nsec / bunker / create) | Needs `nmp_app_signin_nsec` + `nmp_app_signin_bunker` FFI commands; actor-side AccountManager integration |
| Compose (`publish_note`) | Needs `nmp_app_publish_note` FFI; actor needs a PublishEngine instance with Nip65OutboxResolver as its outbox |
| Accounts (multi-session) | ActiveAccountReactor exists in `nmp-signers`; actor needs to execute the command bundle |
| NoteDetail (replies + likes) | Needs `nmp_app_react` FFI + reply-tree projection |
| Keychain at-rest secret storage | Filed as T63a per the original task brief |

## What did land (this PR)

- `crates/nmp-core/src/publish/nip65.rs` — Nip65OutboxResolver (kind:10002
  → write/read relays per NIP-65).
- `crates/nmp-signers/src/identity/active_account_reactor.rs` —
  observer + atomic command bundle for active-account transitions.
- `crates/nmp-testing/tests/real_relay_smoke.rs` — real-relay smoke tests
  (kind:1 round-trip via `wss://relay.damus.io`, outbox resolver against
  realistic kind:10002 input).
- `crates/nmp-signers/examples/gen_nsec.rs` + `fixtures/test_nsec.txt`.
- This iOS scaffold.

QA agent: walk the demo above in simulator. The physical-iPhone install
is the user's manual step (see `INSTALL-ON-IPHONE.md`).
