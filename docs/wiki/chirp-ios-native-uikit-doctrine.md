---
title: Chirp iOS Native UIKit Doctrine
slug: chirp-ios-native-uikit-doctrine
summary: The entire Chirp iOS app must use only typical native iOS controls with semantic names, no hardcoded colors or custom styles
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-19
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:5d893073-9635-450b-b8e9-50648bc1a4e7
---

# Chirp iOS Native UIKit Doctrine

## Native UIKit-Style Controls & Semantic Design

The entire Chirp iOS app must use only typical native iOS controls with semantic names, no hardcoded colors or custom styles. ChirpTheme.swift tokens must map to native iOS semantic values (Color.accentColor, Color.primary, Color.secondary, Color.tertiaryLabel, Color.separator, standard Font modifiers) instead of custom violet/rounded values. [^5d893-1]


GlassCard must be a plain padding wrapper, ChirpPrimaryButton must be a plain Button, ChirpSectionHeader must be Text(title).font(.caption), and ChirpAvatar must use plain Circle().fill(Color.secondary.opacity(0.2)). [^5d893-2]

The foreground style .accent is not a valid ShapeStyle in SwiftUI; Color.accentColor must be used instead. [^5d893-3]

No hardcoded Color.black, Color.blue, Color.indigo, Color.purple, or other non-semantic colors may remain in Swift view files. [^5d893-4]

SwiftUI views must not use .listRowBackground(Color.clear), .listRowSeparator(.hidden), .scrollContentBackground(.hidden), or .listStyle(.plain) to override native list appearance. [^5d893-5]

Settings views must use native iOS Form with Section headers and NavigationLink with Label, not custom scrollviews. [^5d893-6]

RelaySettingsView.swift must use native Form, standard NavigationStack sheet, and semantic colors instead of Color.purple, GlassCard, ChirpPrimaryButton, capsule badges, and custom row backgrounds. [^5d893-7]

NoteContentView.swift must use Color(.secondarySystemBackground) for video card backgrounds and Color.accentColor for URLs and mentions, instead of Color.black.opacity(0.72), Color.blue, and Color.indigo. [^5d893-8]
## See Also

