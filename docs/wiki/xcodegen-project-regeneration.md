---
title: XcodeGen Project Regeneration — Never Hand-Edit project.pbxproj
slug: xcodegen-project-regeneration
summary: Chirp iOS uses XcodeGen; project.pbxproj is generated from project.yml and must be regenerated (never hand-edited) whenever Swift source files are added.
tags:
  - ios
  - xcodegen
  - build
  - project
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
---

# XcodeGen Project Regeneration — Never Hand-Edit project.pbxproj

> Chirp iOS uses XcodeGen; project.pbxproj is generated from project.yml and must be regenerated (never hand-edited) whenever Swift source files are added.

## Build System

Chirp iOS uses XcodeGen with `project.yml` as the project specification. `project.yml` uses `sources: - path: Chirp` — XcodeGen auto-includes all Swift files recursively under the Chirp directory. The `project.pbxproj` is a generated artifact and must never be edited directly. [^9a2c7-17]

## Regeneration Requirement

Any PR that adds new Swift source files to the Chirp directory must run `xcodegen generate` to regenerate `project.pbxproj`. Failing to do so leaves the files on disk but absent from the Xcode project, causing build failures with "Cannot find type in scope" errors for types defined in those files. The specific incident involved PRs #755 and #762 adding three generated Swift files (`ContentTree.generated.swift`, `OpFeedSnapshot.generated.swift`, `FeedWindow.generated.swift`) without regenerating the project. [^9a2c7-18]

## Diagnosis of Missing Files

Build failures from missing project files manifest as Swift compilation errors about types defined in `.generated.swift` files. The fix is to run `xcodegen generate`, not to manually edit `project.pbxproj`. Manual edits to `project.pbxproj` (adding UUIDs, file references, build file entries) should be reverted in favor of regeneration. [^9a2c7-19]

## See Also
- [[chirp-ios-rust-library-build|Chirp iOS Rust Library Build — Feature Flags and Linkage]] — related guide
- [[chirp-ios-simulator|Chirp iOS Simulator — Dedicated Device and Launch Procedure]] — related guide
- [[op-centric-home-feed|OP-Centric Home Feed (V-80) — Architecture and Status]] — related guide

