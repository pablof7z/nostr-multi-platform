---
title: Chirp iOS Simulator — Xcode 26 Beta Workarounds and Framework Paths
slug: chirp-ios-simulator-workaround
summary: The Chirp iOS project.yml must include ENABLE_DEBUG_DYLIB=NO and a simulator-scoped FRAMEWORK_SEARCH_PATHS pointing to /tmp/LocalFrameworks to work around Xcode
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-05-27
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:485a5310-d073-41c9-b230-e6e77926a143
---

# Chirp iOS Simulator — Xcode 26 Beta Workarounds and Framework Paths

## Build Configuration Workaround

The Chirp iOS project.yml must include ENABLE_DEBUG_DYLIB=NO and a simulator-scoped FRAMEWORK_SEARCH_PATHS pointing to /tmp/LocalFrameworks to work around Xcode 26 beta linker restrictions on SwiftUICore and UIUtilities. [^485a5-2]


The /tmp/LocalFrameworks directory must contain patched SwiftUICore.tbd (allowable-clients block removed) and UIUtilities.tbd (minimal stub) before building for the simulator. [^485a5-3]
## See Also

