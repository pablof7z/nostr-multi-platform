# ADR-0048 NIP-55 Amber E2E Runbook

Acceptance procedure for the Stage-4 emulator E2E that gates the `compose/login-block`
registry entry from `soon` to `stable`. Run this whenever the NIP-55 stack is touched.

## Prerequisites

| Item | Requirement |
|------|-------------|
| Android emulator | arm64-v8a AVD, API 35+ (e.g. `TenexOffTablet`) |
| Amber APK | v6.x installed on the emulator (`com.greenart7c3.nostrsigner`) |
| Test key | Any valid nsec imported into Amber (see § Key setup below) |
| NMP Gallery APK | Built with `--features android-ffi` native lib + auth section in registry.json |
| adb | Connected (`adb devices` shows `device`) |
| cargo-ndk | Available for native rebuild if needed |

### Key setup

Generate a fresh key with `nak key generate` and import the nsec into Amber. Record the
pubkey hex for verification:

```
nsec1ze6wve3hadjs0wjghvy3na622a95r24ah5z3lgwk6tlr836r7ytqerhh88
pubkey: 740998438b8f8e308d65554ca4d980357382cf668e57feaad75816c1beabdd27
```

## Build

```sh
# Native lib (only when Rust sources change)
cd apps/nmp-gallery/nmp-app-gallery
cargo ndk -t arm64-v8a -o ../android/app/src/main/jniLibs build \
  --features android-ffi --release
cd ..

# APK (always after any Kotlin or registry.json change)
cd android && ./gradlew :app:assembleDebug && cd ..

# Install
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
```

The gallery splash screen takes 15–20 seconds on a cold emulator start due to ART JIT
verification. Wait for `Displayed org.nmp.gallery/.MainActivity` in logcat before
tapping.

## E2E procedure

### 1. Clear state and start logcat

```sh
adb shell am force-stop com.greenart7c3.nostrsigner
adb shell am force-stop org.nmp.gallery
adb logcat -c
adb logcat -v time > /tmp/nip55_e2e.txt &
```

### 2. Launch gallery

```sh
adb shell am start -n "org.nmp.gallery/.MainActivity"
# Wait until logcat shows: Displayed org.nmp.gallery/.MainActivity (~15-20s)
```

### 3. Navigate to NostrLoginBlock

Gallery main list → tap **Auth** (row at y≈256 physical) → tap **NostrLoginBlock**
(same row position in the Auth section list) → verify "Sign in with Amber" card is
visible with the Amber icon.

### 4. Tap Sign in with Amber

Tap the Amber card. The gallery fires a `nostrsigner:` Intent with:
- `extras["type"] = "get_public_key"` (mandatory — the Stage-4 fix)
- `extras["returnType"] = "signature"`
- `extras["permissions"] = [{"type":"sign_event","kind":1},...]`

Amber's `SignerActivity` must open within ~1 second and show:
- App name: **NMP Gallery** / `org.nmp.gallery`
- Account: the npub of your imported key
- Permissions: Approve basic actions (pre-selected)

**Act within 10 seconds** — Amber times out unapproved requests after `PT10S`.

### 5. Approve in Amber

Tap **Connect**. Allow the Android notification permission if prompted first.

### 6. Verify result in logcat

```sh
grep "740998" /tmp/nip55_e2e.txt  # or your test pubkey hex
```

Expected: Amber's `NotificationSubscription` queries relays for your pubkey's
kind:0 profile immediately after Connect, proving `get_public_key` returned the
correct pubkey to the gallery's kernel.

The gallery's `signer_state` projection should transition to `ready` state carrying
the pubkey-only `SignerPayload::Nip55`.

### 7. Pass/fail criteria

| Check | Pass | Fail |
|-------|------|------|
| Amber opens | `SignerActivity` shows NMP Gallery + correct npub | "Invalid request: Amber received a malformed nostrsigner request" |
| Pubkey returned | Logcat shows relay subscription with test pubkey | No subscription, or `user cancelled` in bridge log |
| No signer error | Gallery returns to signer-state without error toast | Error toast / `Unavailable` outcome |

## Known issues

- **Amber 10s timeout**: Amber auto-rejects the `get_public_key` request if the
  user takes more than 10 seconds from Intent dispatch to tapping Connect. This
  only affects the initial first-time approval; once the app is approved and
  permissions are granted Amber uses the ContentResolver fast-path (no timeout).

- **Gallery 15s cold start**: The emulator JIT verification pass causes a 15–20s
  blank screen on first launch after install. This is normal; wait for
  `Displayed` in logcat before navigating.

- **Amber notification permission**: On first launch Amber asks for Android
  `POST_NOTIFICATIONS` permission before showing the Connect dialog. Tap Allow.
  This dialog doesn't count against the 10s timeout.

## Stage-4 pass record

Passed 2026-06-12 by agent `agent-a43634388073b8a7c` on `TenexOffTablet` emulator
(arm64, API 35, 2560x1600 @ 320dpi):

- Root cause of pre-fix failure: `dispatchIntent` built URI
  `nostrsigner:get_public_key?type=get_public_key&permissions=[...]` — Amber reads
  `type` from `intent.extras`, not the URI query string, so received `null` type →
  `SignerType.INVALID` → "malformed nostrsigner request".
- Fix: URI simplified to `nostrsigner:`, all parameters moved to Intent extras
  (`type`, `returnType`, `payload`, `current_user`, `pubkey`, `permissions`).
- Permissions format fixed: `"sign_event:1"` → `{"type":"sign_event","kind":1}`.
- Evidence: Amber showed NMP Gallery approval dialog + pubkey
  `740998438b8f8e308d65554ca4d980357382cf668e57feaad75816c1beabdd27`. Amber
  queried relays for that pubkey immediately after first Connect tap (logcat:
  `NotificationSubscription onSend`). No "Invalid request" error.
