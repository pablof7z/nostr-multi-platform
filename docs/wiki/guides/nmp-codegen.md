---
title: NMP Codegen
slug: nmp-codegen
topic: ffi-runtime
summary: nmp-codegen has 1,876 LOC but emits nothing; KernelBridge.swift is 1,988 LOC handwritten as its counterpart
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-06-18
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:019edc01-fdde-7b20-a348-5a2a9ce1a0f9
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# NMP Codegen

## Overview

nmp-codegen has 1,876 LOC but emits nothing; KernelBridge.swift is 1,988 LOC handwritten as its counterpart. Four platforms exist: iOS (Swift/SwiftUI), Android (Kotlin/Compose), Web (TypeScript/React), and TUI (Rust). P9 PR2 is executed as phased: PR2a closes the drift bug with catalog+gate, and PR2b (#1576) adds nmp-codegen emitters that generate native Kotlin×3/Swift×2 known-signer lists + AndroidManifest <queries> + iOS plist schemes from the Rust catalog, retiring the hand-parse gate and making Rust the codegen-enforced sole writer. The phased approach (rather than monolithic) is necessary because the native list is an embedded section in triple-vendored multi-purpose files and Swift copies already drift.

<!-- citations: [^95d02-14] [^86221-7] [^019ed-24] [^11850-191] -->
