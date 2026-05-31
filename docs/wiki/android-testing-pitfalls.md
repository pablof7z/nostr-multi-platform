---
title: Android Testing Pitfalls — Gesture Nav, Onboarding, and Account Creation
slug: android-testing-pitfalls
summary: Gesture nav intercepts tab bar taps; switch to 3-button nav. Onboarding is the expected fresh-install state. Account creation verified working.
tags:
  - android
  - testing
  - emulator
  - qa
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Android Testing Pitfalls — Gesture Nav, Onboarding, and Account Creation

> Gesture nav intercepts tab bar taps; switch to 3-button nav. Onboarding is the expected fresh-install state. Account creation verified working.

## Gesture Navigation Conflict

Android's gesture navigation bar intercepts taps near the bottom of the screen (approximately y=2232 on a 1080x2400 device). This overlaps with the tab bar in Chirp Android, causing tab taps to be consumed by the system gesture zone instead of reaching the app's tab bar. When taps on the tab bar produce no response, the app appears unresponsive even though the kernel is alive and the feed is rendering correctly. [^4edd4-41]

## Workaround

Switch the Android device from gesture navigation to 3-button navigation. This eliminates the gesture zone overlap and allows tab bar taps to register correctly. The switch is done via the system Settings app: System > Gestures > System navigation > 3-button navigation. After switching, all tabs (Home, DMs, Notifications, Relays, Profile) become tappable. [^4edd4-42]

## Testing Impact

Automated testing agents (Haiku agents) using screenshot-based tap coordinates may report the app as unresponsive when the real issue is gesture nav interception. Before diagnosing an Android UI as frozen, verify the navigation mode: check whether taps at the bottom 5% of the screen are being consumed by gesture nav. Use `adb shell uiautomator dump` to get exact element positions and confirm whether the tab bar buttons are within the gesture zone. [^4edd4-43]

## Onboarding and Account Creation

On a fresh Android install with no stored session, Chirp displays the onboarding screen. Tapping "Create local account" creates an account via `nativeCreateLocalAccount` (which calls `nmp_app_create_new_account` directly — not through `dispatch_action`). After account creation, the kernel rev counter jumps (e.g., from rev 6 to rev 11+) and the timeline view appears with real feed content. The onboarding screen is the expected initial state and does not indicate a bug. [^4edd4-44]

## See Also
- [[chirp-cross-platform-feature-parity-testing|Chirp Cross-Platform Feature Parity — Mandated Testing Across All Clients]] — related guide
- [[android-relays-tab-duplicate-key-crash|Android Relays Tab — Duplicate Key Crash in LazyColumn]] — related guide
- [[claim-expansion-terminate-claim-invariant|Claim Expansion — terminate_claim Is the Sole Phase::Terminal Transition Point]] — related guide

