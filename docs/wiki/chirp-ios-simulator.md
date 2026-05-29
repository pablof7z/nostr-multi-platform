---
title: Chirp iOS Simulator — Dedicated Device and Launch Procedure
slug: chirp-ios-simulator
summary: Chirp iOS has a dedicated simulator ("Use this for Chirp iOS", UUID 121F34F8-B41E-41F6-B788-2188D183BD97); use it instead of generic iPhone simulators.
tags:
  - ios
  - simulator
  - chirp
  - development
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
---

# Chirp iOS Simulator — Dedicated Device and Launch Procedure

> Chirp iOS has a dedicated simulator ("Use this for Chirp iOS", UUID 121F34F8-B41E-41F6-B788-2188D183BD97); use it instead of generic iPhone simulators.

## Dedicated Simulator

Chirp iOS has a dedicated simulator named "Use this for Chirp iOS" with UUID `121F34F8-B41E-41F6-B788-2188D183BD97`. This is the canonical simulator for Chirp development. Other simulators (e.g., iPhone 17 Pro) may exist in the environment but the Chirp-dedicated one should be used. [^9a2c7-23]

## Simulator State Issues

Simulators can encounter state issues that prevent app installation even after a successful build. When this happens, the fix is to shut down the problematic simulator, boot the Chirp-dedicated simulator, and retry. If the dedicated simulator is shut down, boot it explicitly before attempting to run the app. [^9a2c7-24]

## Onboarding Screen

On first launch, Chirp iOS displays an onboarding screen with Chirp branding. This is the expected initial state for a fresh install on the simulator. [^9a2c7-25]


Onboarding Screen

After a fresh build and install, the timeline may initially appear empty. This is expected — the app needs time to sync with relays and populate the home feed. Wait and take another screenshot rather than treating an empty timeline as a build failure. [^9a2c7-44]
## See Also
- [[xcodegen-project-regeneration|XcodeGen Project Regeneration — Never Hand-Edit project.pbxproj]] — related guide
- [[chirp-ios-rust-library-build|Chirp iOS Rust Library Build — Feature Flags and Linkage]] — related guide

