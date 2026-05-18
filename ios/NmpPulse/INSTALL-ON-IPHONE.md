# Installing NmpPulse on a physical iPhone

The simulator build (see `README.md`) is the QA-agent path. Installing on a
physical iPhone requires a few extra steps because Apple's code-signing
needs a developer team ID. Free Apple-ID provisioning is sufficient — no
paid Apple Developer account required.

## Prerequisites (one-time)

1. **Plug in your iPhone via USB.** (Wireless install also works once
   debugging-over-Wi-Fi is enabled in Xcode → Devices.)
2. **Sign in to Xcode with your Apple ID.** Xcode → Settings →
   Accounts → "+". The free tier provisions for 7-day install cycles —
   you'll re-install once a week. Sufficient for validation.
3. **Trust the developer profile on the iPhone** (after first install):
   Settings → General → VPN & Device Management → tap the profile under
   "DEVELOPER APP" → Trust.

## Discover your device

```bash
xcrun devicectl list devices
```

Note the UUID-like identifier (e.g. `00008101-001A12345678001E`) and the
device name (e.g. `Pablo's iPhone`).

## Build for device

```bash
# 1. Build the device static lib (real iPhone, ARM64 device target).
cargo build -p nmp-core --target aarch64-apple-ios

# 2. Build the app — Xcode handles provisioning automatically.
cd ios/NmpPulse
xcodebuild -project NmpPulse.xcodeproj \
  -scheme NmpPulse \
  -destination 'platform=iOS,name=<YOUR-IPHONE-NAME>' \
  -configuration Debug \
  -derivedDataPath ./build \
  -allowProvisioningUpdates \
  build
```

If `xcodebuild` complains about `DEVELOPMENT_TEAM`, open the project in
Xcode once, select the NmpPulse target → Signing & Capabilities → pick
your personal team from the dropdown. `CODE_SIGN_STYLE: Automatic` in
`project.yml` means Xcode handles provisioning from there.

## Install + launch

```bash
APP_PATH="./build/Build/Products/Debug-iphoneos/NmpPulse.app"
xcrun devicectl device install app --device <UDID> "$APP_PATH"
xcrun devicectl device process launch --device <UDID> com.example.NmpPulse
```

## Device-log capture

```bash
xcrun devicectl device console --device <UDID> \
  | grep -i nmp > /tmp/pulse-device.log
```

## Troubleshooting

- **"Untrusted Developer" alert on device.** Settings → General → VPN &
  Device Management → Trust the dev profile. Re-run launch.
- **`devicectl` flow fails.** Fall back to Xcode GUI: open
  `NmpPulse.xcodeproj`, pick the physical device as destination, hit Run.
- **`-allowProvisioningUpdates` says no team.** You haven't signed Xcode
  into an Apple ID yet (Xcode → Settings → Accounts).
- **App installs but crashes immediately.** Check console log for
  signature / entitlement errors. Free-tier provisioning has a 7-day
  expiration; re-install if it's been a week since last build.

## What you'll see on the iPhone

Same as the simulator (see `README.md`): a Timeline tab with live notes
from the kernel-bootstrap pubkey, a Diagnostics tab, and a More tab
listing the scope-deferred features (T66a).

The Onboarding / Compose / Accounts flows are not yet wired into the FFI
surface — installing on iPhone today shows the Timeline-only scaffold.
The user-experience parity between sim and device is exact.
