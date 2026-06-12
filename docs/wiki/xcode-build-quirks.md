---
title: Xcode Build and Code Generation Quirks
slug: xcode-build-quirks
topic: mobile-ci
summary: "The BuildInfo.generated.swift xcodegen quirk requires a two-step build: build once to generate BuildInfo, re-run xcodegen, then build again"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-09
updated: 2026-06-09
verified: 2026-06-09
compiled-from: conversation
sources:
  - session:63af4b96-d3d3-45c3-ab96-9f899beafa1b
---

# Xcode Build and Code Generation Quirks

## BuildInfo.generated.swift Two-Step Build

The BuildInfo.generated.swift xcodegen quirk requires a two-step build: build once to generate BuildInfo, re-run xcodegen, then build again. Additionally, the committed pbxproj must exclude BuildInfo to match master's clean-checkout convention. <!-- [^63af4-6] -->
