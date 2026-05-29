---
title: CI FlatBuffers Version-Pin Script Must Cover All Gradle Files
slug: android-ci-pin-gap-app-vs-gallery
summary: ci/check-flatbuffers-version-pins.sh must be updated whenever a new gradle file pins a FlatBuffers version; omitting any file leaves that pin unguarded.
tags:
  - android
  - ci
  - flatbuffers
  - gradle
  - config
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:cd331450-f93f-48d0-960e-3c73e927775e
---

# CI FlatBuffers Version-Pin Script Must Cover All Gradle Files

> `ci/check-flatbuffers-version-pins.sh` was originally written to check only the gallery's `build.gradle`. When the active app (`android/app/build.gradle.kts`) added its own FlatBuffers pin, the script was not updated, leaving that pin completely unguarded by CI.

## Details

- **Rule:** Whenever a FlatBuffers version pin is added to *any* gradle file in the repo, a corresponding `require_line` (or equivalent assertion) must be added to `ci/check-flatbuffers-version-pins.sh` in the same commit.
- The script must be treated as an exhaustive registry of all gradle-level FlatBuffers pins, not a snapshot of the files that existed at script-creation time.
- During code review, verify that the set of gradle files checked by the script matches the set of gradle files that contain FlatBuffers dependencies. A mismatch is a CI gap, not a deferred task.
- This pattern generalises: any version-pin enforcement script should be audited for coverage whenever a new dependency site is added.


### Additional Rule

## Manual Discovery Required for New Android Modules

When adding a new FlatBuffers dependency to **any** Android module (not just the gallery), verify that `ci/check-flatbuffers-version-pins.sh` explicitly covers that module's `build.gradle.kts`. The script does **not** auto-discover gradle files — each file must be added manually. The original gap was that `android/app/build.gradle.kts` was not covered while the gallery's gradle file was.
## See Also
- [[flatbuffers-kotlin-version-pin|flatbuffers kotlin version pin]] — related guide
- [[flatbuffers-kotlin-version-pin|flatbuffers kotlin version pin]] — related guide

- [flatbuffers-kotlin-version-pin](flatbuffers-kotlin-version-pin)
