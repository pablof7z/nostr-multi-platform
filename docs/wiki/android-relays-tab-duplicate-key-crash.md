---
title: Android Relays Tab — Duplicate Key Crash in LazyColumn
slug: android-relays-tab-duplicate-key-crash
summary: The Relays tab crashes when the same relay URL appears with different roles; the LazyColumn key must include the role to guarantee uniqueness.
tags:
  - android
  - relays
  - compose
  - crash
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-30
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Android Relays Tab — Duplicate Key Crash in LazyColumn

> The Relays tab crashes when the same relay URL appears with different roles; the LazyColumn key must include the role to guarantee uniqueness.

## Crash Description

The Android Relays tab crashes with a duplicate key exception in the Compose LazyColumn when the same relay URL appears with multiple roles (e.g., read and write). The crash message reports `java.lang.IllegalArgumentException: Key "wss://relay.primal.net" was already used. If you are using LazyColumn/Row, please make sure you provide a unique key for each item.` [^4edd4-37]

## Root Cause

The LazyColumn in the Relays tab uses `key = { it.relayUrl }` as the item key. When the same relay URL (e.g., `wss://relay.primal.net`) appears twice in the relay list — once with a read role and once with a write role — two items share the same key, violating Compose's uniqueness requirement. [^4edd4-38]

## Fix

Use a composite key that includes the relay role alongside the URL: `key = { "${it.role}:${it.relayUrl}" }`. This guarantees uniqueness because a relay URL combined with its role is always distinct, even when the same URL appears with different roles. The Diagnostics screen already uses this correct pattern and serves as the reference implementation. [^4edd4-39]


This fix was delivered as PR #814, alongside the kernel panic fix (PR #810) and iOS repost wiring (PR #811), as part of the cross-platform feature parity validation session. [^4edd4-81]
## Verification

After applying the fix, rebuild and reinstall the Android APK. Navigate to the Relays tab to confirm it loads without crashing. Test that all other tabs (Home, DMs, Notifications, Profile) continue to work correctly. [^4edd4-40]

## See Also
- [[chirp-cross-platform-feature-parity-testing|Chirp Cross-Platform Feature Parity — Mandated Testing Across All Clients]] — related guide
- [[android-testing-pitfalls|Android Testing Pitfalls — Gesture Nav, Onboarding, and Account Creation]] — related guide
- [[flatbuffers-codingkey-rawvalue-camelcase|FlatBuffers CodingKey rawValues Must Be camelCase — convertFromSnakeCase Mismatch]] — related guide

