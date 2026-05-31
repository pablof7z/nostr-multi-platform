---
title: Chirp Cross-Platform Feature Parity — Mandated Testing Across All Clients
slug: chirp-cross-platform-feature-parity-testing
summary: All three clients (iOS, Android, TUI) must be tested in parallel with Haiku agents covering avatars, names, social actions, marmot, and every normal Nostr client feature — fix it, don't hack it.
tags:
  - testing
  - cross-platform
  - parity
  - haiku
  - ios
  - android
  - tui
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-18
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:29d2c220-a86b-4b0d-82fb-d40d8fd4505e
---

# Chirp Cross-Platform Feature Parity — Mandated Testing Across All Clients

> All three clients (iOS, Android, TUI) must be tested in parallel with Haiku agents covering avatars, names, social actions, marmot, and every normal Nostr client feature — fix it, don't hack it.

## Mandate

All three Chirp clients (iOS, Android, TUI) must maintain feature parity since they share the same Rust codebase — the UI is just there to render stuff. The Android Nostr entity renderer must achieve feature parity with the iOS implementation, including an equivalent standalone showcase app. When validating any change, run a battery of tests across all three clients using Haiku agents, checking that everything that should work in a normal Nostr client actually works: avatars rendering, all names always rendering everywhere, the ability to reply, follow, unfollow, and any other standard social-media-client feature including marmot support. If something that should work doesn't, fix it — don't hack it. [^4edd4-11]

<!-- citations: [^4edd4-11] [^4edd4-36] [^29d2c-1] -->
## Testing Scope

A comprehensive test battery must cover: avatar rendering (all avatars display correctly for all users), name rendering (display names and usernames render everywhere without truncation or missing fields), social actions (reply to posts, follow users, unfollow users), marmot/MLS support across all clients, and any other standard feature of a normal Nostr/twitter client. The test agent should use an Opus agent to produce a guide of everything to try, then a Haiku agent to execute the tests. The Android Identicon must use a Compose Canvas implementation with a byte-for-byte identical algorithm to iOS.

The Marmot 3-client interop test uses `chirp-repl` (headless) instead of the TUI (which cannot run as a subprocess) while sharing the same Rust Marmot runtime, so a headless success proves the TUI runtime works.

Display names appearing as hex pubkeys (instead of human-readable names) on a fresh install is normal Nostr behavior, not a bug. Profile resolution is on-demand: each rendered card triggers a lazy `claim_profile` fetch, and display names resolve as kind:0 metadata events arrive from relays. On a fresh install with no cached profiles, hex pubkeys may persist for several seconds until relay fetches complete. This affects all platforms equally since it is driven by the shared Rust kernel, not the UI layer. Some pubkeys may remain unresolved if their kind:0 events are not available from the connected relays. [^4edd4-30]

When a Haiku testing agent reports a button as non-responsive, verify the agent tapped the correct UI element. In one case, the agent reported the compose button as not working but had actually tapped the paperplane (outbox) icon. The compose button was confirmed to be correctly wired via `showCompose = true`. Always cross-check agent-reported UI bugs against the actual view hierarchy before implementing a fix. [^4edd4-34]

Tab navigation testing uncovered an Android-specific crash on the Relays tab: when the same relay URL appears with different roles (e.g., read and write), the Compose LazyColumn crashes with a duplicate key exception. The fix uses a composite key `"${it.role}:${it.relayUrl}"` matching the pattern already used in the Diagnostics screen. This bug is Android-only because iOS uses a different list component that handles duplicates differently. [^4edd4-46]

The comprehensive test battery for this session covered: iOS — all 5 tabs, feed loads with content, like/reaction, reply compose sheet, repost functionality, wallet NWC UI, thread view, profile view, no crash banner; Android — all 6 tabs (Timeline, DMs, Relays, Account, Wallet, Diagnostics), account creation, relay list without crashing, kernel alive throughout (rev 6→15), real timeline events; TUI — builds and links cleanly, stored keychain session exists, but cannot be Haiku-tested because enable_raw_mode() requires a real TTY (environment constraint, not a code bug). [^4edd4-78]

The compose button in Chirp iOS is correctly wired (showCompose = true) and functional. A Haiku testing agent initially reported it as non-responsive, but investigation revealed the agent had tapped the paperplane (outbox) icon by mistake instead of the compose button. Always cross-check agent-reported UI bugs against the actual view hierarchy before implementing a fix. [^4edd4-80]

The cross-platform validation session produced 3 fixes across 3 PRs: PR #810 (Rust kernel claim_sub_index panic + FlatBuffers primaryId decode failure), PR #811 (iOS repost NIP-18 fully wired), and PR #814 (Android Relays tab duplicate-key crash). Haiku-driven validation confirmed: iOS — all 5 tabs functional, feed loads, like/reply/repost/wallet all working; Android — all 6 tabs functional, account creation, relay list crash-free, kernel alive; TUI — builds and links, requires real TTY for testing. [^4edd4-131]

<!-- citations: [^4edd4-12] [^4edd4-30] [^4edd4-34] [^4edd4-46] [^4edd4-78] [^4edd4-80] [^4edd4-131] [^4edd4-210] [^29d2c-2] -->
## Parallel Testing Pattern

Testing agents run in parallel across feature areas. Example fan-out: one Haiku agent tests home feed, profiles, and navigation; another tests social actions (like, reply, compose). While those run, the orchestrator can check other platform states (TUI build status, Android build status) since core fixes benefit all clients. Two agents sharing a simulator may have interleaved interactions — findings are consolidated when both complete. [^4edd4-13]


Android UI testing agents may use image pixel coordinates instead of device coordinates, causing taps to miss their targets. When the agent reports the UI as "frozen" but the kernel rev counter is still incrementing (indicating the app is alive), verify coordinate mapping: the Android emulator uses device coordinates, and screenshots may be displayed at a different scale. Use uiautomator (`adb shell uiautomator dump`) to get exact element positions instead of relying on pixel coordinates from scaled screenshots. [^4edd4-26]

The TUI cannot be tested in an automated subprocess because `enable_raw_mode()` requires a real TTY. The TUI builds cleanly and can store keychain sessions, but it must be launched in a real terminal window to operate. This is an environment constraint, not a code bug. TUI testing in the cross-platform battery is therefore limited to build verification and static analysis. [^4edd4-29]

Android gesture navigation can intercept taps on the bottom tab bar, making the app appear frozen to automated testing agents. The gesture nav zone occupies approximately the bottom 5% of the screen and overlaps with the tab bar. Switching to 3-button navigation (System > Gestures > System navigation > 3-button navigation) resolves this. Always verify the navigation mode before concluding the Android app is unresponsive. [^4edd4-45]

When a Haiku testing agent reports the Android kernel as "frozen" (rev counter stuck), verify the agent's coordinate mapping first. In one session, the agent used scaled screenshot pixel coordinates instead of device coordinates — the kernel was actually alive (rev jumped 6→11→13) but the agent was tapping wrong screen locations. Always use uiautomator (adb shell uiautomator dump) for exact element positions. [^4edd4-105]
## Crash Recovery UX

When the kernel (background service) stops unexpectedly — such as from a Rust panic — Chirp iOS displays the message "Background service stopped / Please relaunch the app to recover." This is the user-facing surface for any unhandled kernel failure. The message persists until the app is killed and relaunched. [^4edd4-14]

## Kernel Liveness Check

The kernel's `isAlive()` method returns true before `start()` is called, so a pre-start liveness check will not detect a kernel that is going to die during startup. The kernel can die during or after startup due to panics in core logic — logs must be captured to identify the root cause. [^4edd4-15]


On Android (release mode), `debug_assert` statements do not fire. A kernel bug that causes a panic on iOS debug builds may manifest on Android as a silent stall — the kernel rev counter stops incrementing but no crash message appears. When the Android kernel appears "frozen" at a specific rev, check whether the same code path has a `debug_assert` that panics on iOS debug builds. The fix applied to `nmp-core` benefits all platforms, but the Android APK must be rebuilt with the fixed Rust library to pick up the change. [^4edd4-27]
## See Also
- [[cross-platform-qa-code-review-workflow|Cross-Platform QA and Code-Review Fan-Out — Build, Run, Review, Synthesize]] — related guide
- [[chirp-cross-platform-parity-plan|Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work]] — related guide
- [[claim-expansion-terminate-claim-invariant|Claim Expansion — terminate_claim Is the Sole Phase::Terminal Transition Point]] — related guide
- [[flatbuffers-codingkey-rawvalue-camelcase|FlatBuffers CodingKey rawValues Must Be camelCase — convertFromSnakeCase Mismatch]] — related guide
- [[chirp-ios-repost-nip18|Chirp iOS Repost (NIP-18) — Implementation and Wiring]] — related guide
- [[android-relays-tab-duplicate-key-crash|Android Relays Tab — Duplicate Key Crash in LazyColumn]] — related guide
- [[android-testing-pitfalls|Android Testing Pitfalls — Gesture Nav, Onboarding, and Account Creation]] — related guide

