---
title: Chirp iOS BuildInfo.generated.swift — Pre-Build Script and Welcome Screen Display
slug: chirp-ios-build-info-generated
summary: The welcome screen displays the current git branch, commit hash, and build time at the bottom, populated automatically via a pre-build script without manual upd
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-26
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:0048057e-cb95-4da0-9f74-039a07dfc89f
---

# Chirp iOS BuildInfo.generated.swift — Pre-Build Script and Welcome Screen Display

## Welcome Screen Build Info

The welcome screen displays the current git branch, commit hash, and build time at the bottom, populated automatically via a pre-build script without manual updates. [^00480-1]


project.yml includes a preBuildScripts phase with basedOnDependencyAnalysis: false that shells out to git for branch/commit and date for build time to write BuildInfo.generated.swift. [^00480-2]

BuildInfo.generated.swift has a committed placeholder file so xcodegen picks it up, and its generated values are excluded from source control via .gitignore. [^00480-3]

The welcome screen build info footer uses .safeAreaInset(edge: .bottom) to pin text above the home indicator without modifying the existing VStack layout, avoiding a SwiftUI layout crash on iOS 26 beta. [^00480-4]
## See Also

