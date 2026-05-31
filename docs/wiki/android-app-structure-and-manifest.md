---
title: Android App Structure and Manifest
slug: android-app-structure-and-manifest
summary: The Android app package name is `org.nmp.android` with `minSdk 26`, targeting Android 8.0+.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:e2d58641-a6c3-4f43-94c0-b018c8fbb893
---

# Android App Structure and Manifest

## App Identity & Configuration

The Android app package name is `org.nmp.android` with `minSdk 26`, targeting Android 8.0+. [^e2d58-2]


The app is labeled 'Chirp' (not 'NmpPulse' or 'Pulse') in the manifest. [^e2d58-3]

## UI & Navigation

The app uses Jetpack Compose + Material 3 for the UI. [^e2d58-4]

The app uses Navigation Compose for typed navigation with `NavHost` and `NavController`. [^e2d58-5]

## Data & Networking

The app uses Coil for async image loading of avatars. [^e2d58-6]

The app uses `kotlinx-serialization` for JSON decoding of kernel snapshots. [^e2d58-7]

## Content Constraints

The ComposeSheet has a 280-character limit on note content. [^e2d58-8]
## See Also

