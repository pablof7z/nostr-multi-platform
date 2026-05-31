---
title: iOS SwiftUI Idle Re-Render Elimination (C3)
slug: ios-swiftui-idle-re-render
summary: Eliminating the iOS idle re-render of approximately four unchanged rows per second is the top priority for the next fix.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-31
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:c9ae5a7c-0f5e-44ec-94d6-d9b5e31d8991
---

# iOS SwiftUI Idle Re-Render Elimination (C3)

## Priority

Eliminating the iOS idle re-render of approximately four unchanged rows per second is the top priority for the next fix. [^c9ae5-1]


## Layer 1: Row-Level Short-Circuit (PR #880)

PR #880 introduces `rendersIdentically` and a setter guard so that only rows with changed data trigger SwiftUI re-evaluation. `rendersIdentically` compares all 13 visible fields, including `relayCount`. `relayCount` must be included in equality checks because `NoteRowView.relayChip` visually renders it; excluding it would show stale counts. PR #880 includes 6 unit tests: 1 positive and 5 negative controls covering `relayCount`, `content`, `authorDisplayName`, `authorPictureUrl`, and `createdAt`. [^c9ae5-2]

## Whole-KernelUpdate Guard Limitation

A whole-KernelUpdate guard checking if `update == snapshot` must not be attempted because `KernelUpdate.metrics` changes every tick, so the guard would never fire. [^c9ae5-3]

## Layer 2: @Observable Migration

Layer 2 requires migrating `KernelModel` from `ObservableObject`/`@Published` to the `@Observable` macro, splitting the single snapshot slot into per-concern slots (feed, profile, metrics, relays). With `@Observable`, SwiftUI tracks per-property reads so a metrics-only tick only invalidates `DiagnosticsView` and never `HomeFeedView`. [^c9ae5-4]

## Codegen: Shared Render Identity

TimelineItem+RenderIdentity.swift is currently Chirp-specific and lives in ios/Chirp/Chirp/Bridge/. Another app built on NMP would need to manually copy the `rendersIdentically` pattern to get the row-level benefit. The C3 idle re-render fix must be moved from the Chirp app shell into the NMP framework codegen so that all NMP apps benefit automatically without hand-rolling the pattern. (Previously: the fix lived in the Chirp app shell.) The codegen for `KernelTypes.generated.swift` must emit either a complete field-by-field `Equatable` conformance or a `rendersIdentically(other:)` method so any app using `ForEach` over snapshot rows gets idle re-render short-circuiting for free. The scaffolded app's `ForEach` over snapshot rows must automatically use `.equatable()` so SwiftUI never re-evaluates a body when the synthesized `==` returns true. Combining codegen-emitted render identity with the Layer 2 KernelModel `@Observable` migration means any app on NMP gets zero idle re-renders without writing optimization code themselves.

After merging the codegen fix, the C3 fix must be demonstrably present in the scaffold output via a codegen test or Swift unit test proving generated types diff correctly. [^c9ae5-22]

<!-- citations: [^c9ae5-5] [^c9ae5-21] -->
## See Also

