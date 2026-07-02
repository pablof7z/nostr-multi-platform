# ADR-0072 NIP-55 Amber E2E Runbook

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
cd apps/nmp-gallery/crates/nmp-app-gallery
cargo ndk -t arm64-v8a -o ../../android/app/src/main/jniLibs build \
  --features android-ffi --release
cd ../../android

# APK (always after any Kotlin or registry.json change)
./gradlew :app:assembleDebug

# Install
adb install -r app/build/outputs/apk/debug/app-debug.apk
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

### 7. Publish leg — kind:1 through the full kernel pipeline (external Chirp)

The gallery showcase has no publish surface, so the `sign_event` leg runs on
Chirp Android from the standalone Chirp repository (same vendored bridge, same
kernel):

```sh
cd ../chirp

# Chirp native lib (arm64 only; the marmot feature needs an Android-target
# OpenSSL for sqlcipher and is irrelevant to this leg)
cargo ndk --manifest-path crates/nmp-chirp-android-ffi/Cargo.toml \
  -t arm64-v8a -o android/app/src/main/jniLibs build --release
cd android && ./gradlew :app:assembleDebug -x cargoNdk
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

1. Launch Chirp → **Account** tab → **Sign in with Amber** → in Amber select
   **"I fully trust this application"** → **Connect**. (Full trust makes the
   subsequent `sign_event` auto-approve — no second 10s approval window.)
2. **Timeline** tab — verify "Active account: npub1wsy…" (pubkey-only; no
   nsec in the app).
3. Tap the **+** FAB → type a note → **Publish**. Amber's `SignerActivity`
   opens for about one second (auto-sign) and returns; the note appears in
   the timeline. The kernel verifies id + schnorr signature + pubkey identity
   on the returned event (`parse_signed_event_response`) before accepting it.
4. Fetch the event from the write relay and verify independently:

```sh
nak req -k 1 -a <test-pubkey-hex> --limit 3 wss://relay.primal.net \
  | tee /tmp/published_event.json
nak verify < /tmp/published_event.json   # exit 0 = id + sig valid
```

### 8. Pass/fail criteria

| Leg | Check | Pass | Fail |
|-----|-------|------|------|
| get_public_key | Amber opens | `SignerActivity` shows the app + correct npub | "Invalid request: Amber received a malformed nostrsigner request" |
| get_public_key | Pubkey returned | Account active (pubkey-only) in the app; relay subs carry the test pubkey | No subscription, or `user cancelled` in bridge log |
| sign_event | Amber signs | `SignerActivity` opens ≈1s and auto-returns (full trust) | "Invalid request" dialog, or 90s deadline timeout |
| sign_event | Event verified | `nak verify` passes; `pubkey` == Amber-held key; note renders in timeline | Signature invalid / pubkey mismatch / kernel rejects the reply |

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

Three protocol defects in `ExternalSignerCapabilityBridge.dispatchIntent` /
its result handler, each exposed by a failing emulator round and fixed in-PR:

1. **`type` in URI query string** (get_public_key round): the bridge built
   `nostrsigner:get_public_key?type=get_public_key&permissions=[...]` — Amber
   reads `type` from `intent.extras`, not the URI query string, so received
   `null` type → `SignerType.INVALID` → "malformed nostrsigner request".
   Fix: `type`, `id`, `returnType`, `current_user`, `pubkey`, `permissions`
   as Intent extras. Permissions format also fixed: `"sign_event:1"` →
   `{"type":"sign_event","kind":1}` (`buildAmberPermissionsJsonInternal`).
2. **Payload in an extra** (sign_event round): the unsigned-event JSON went
   into `extras["payload"]`, but Amber reads the payload from the data URI
   (`intent.data.toString().replace("nostrsigner:", "")`); the empty URI made
   `getUnsignedEvent("")` throw → the same "malformed" dialog when publishing.
   Fix: `Uri.parse("nostrsigner:" + Uri.encode(payload))`.
3. **Wrong reply extra for sign_event**: Amber returns the signature hex in
   `result` and the FULL signed-event JSON in `event`; Rust's
   `parse_signed_event_response` needs the complete event. Fix:
   `selectAmberResultValue` prefers the `event` extra for `sign_event`.
   The handler now also honours Amber's `rejected: true` RESULT_OK replies.

get_public_key evidence: Amber approval dialog showed the app + pubkey
`740998438b8f8e308d65554ca4d980357382cf668e57feaad75816c1beabdd27`; after
Connect the Chirp timeline showed "Active account: npub1wsy…nsh0ztw4"
(pubkey-only) and the kernel's mailbox bootstrap subscribed for that author
on the test relay.

sign_event evidence (publish leg, Chirp): note published from the Timeline
composer; Amber auto-signed in ~1.3s (logcat: SignerActivity onCreate
22:27:35.758 → onPause 22:27:37.080, no user interaction — full-trust
policy); the kind:1 landed on `wss://relay.primal.net`:

```json
{"kind":1,"id":"11652d49c99eb95da296636ff31de54270782bd94978c45c448cd5401920a76a",
 "pubkey":"740998438b8f8e308d65554ca4d980357382cf668e57feaad75816c1beabdd27",
 "created_at":1781292455,"tags":[],"content":"NIP-55 Stage-4 E2E signed ",
 "sig":"b00cf3aa80748ec24ebbfe8dd12872c464af97f84c8a5fd71d39e0df58f99c021e6c3e947727b50fd7012e28deb745e416a5ecdb715a112b02ac40c3bc089d86"}
```

`nak verify` passes (id + schnorr signature valid); `pubkey` equals the
Amber-held test key. The kernel had already verified the same event
end-to-end (`parse_signed_event_response`) before accepting it for publish.
