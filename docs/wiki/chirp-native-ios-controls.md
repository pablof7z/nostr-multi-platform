---
title: Chirp Native iOS Controls & Theming
slug: chirp-native-ios-controls
summary: The entire Chirp iOS app must use typical native iOS controls with semantic names
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-29
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:5d893073-9635-450b-b8e9-50648bc1a4e7
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
---

# Chirp Native iOS Controls & Theming

## Native iOS Controls Requirement

The entire Chirp iOS app must use typical native iOS controls with semantic names. No hardcoded colors or styles may be used. No hardcoded colors (Color.purple, Color.blue, Color.indigo, Color.black, etc.) may appear in Chirp Swift files. [^5d893-3]


## Settings Screens

Settings screens must be normal iOS settings screens using Form with native Section headers. [^5d893-4]

## ChirpTheme Token Mappings

ChirpTheme.swift tokens must map to native iOS values:
- ChirpColor.accent → Color.accentColor
- ChirpColor.bg → Color(.systemBackground)
- ChirpColor.surface → Color(.secondarySystemBackground)
- ChirpColor.hairline → Color.separator
- ChirpColor.textPrimary → Color.primary
- ChirpColor.textSecondary → Color.secondary
- ChirpColor.textTertiary → Color(.tertiaryLabel)

ChirpFont tokens must map to standard Font modifiers (e.g., .largeTitle.weight(.bold), .title2.weight(.semibold), .headline, .body, .callout, .caption, .footnote.monospaced()). [^5d893-5]

## Component Neutralization

Custom components must be neutralized to plain native equivalents:
- GlassCard → plain padding wrapper
- ChirpPrimaryButton → plain Button
- ChirpSectionHeader → Text(title).font(.caption)
- ChirpAvatar → NostrAvatar (from nmp-gallery registry)

Inline UI components (NostrUserCard, NostrNpubChip, NostrNip05Badge, NostrRelayList) must eventually be extracted into reusable gallery components, but extraction is deferred to the backlog.

<!-- citations: [^5d893-6] [^9a2c7-5] -->
## Non-Native List Patterns

Non-native list patterns must be removed from all Chirp feature views. This includes .listRowBackground(Color.clear), .listRowSeparator(.hidden), .scrollContentBackground(.hidden), .listStyle(.plain), and .background(ChirpColor.bg). [^5d893-7]

## View-Specific Requirements

ComposeView must use a plain VStack instead of GlassCard and must not use .scrollContentBackground(.hidden) or .background(Color.clear) on TextEditor.

OnboardingView must use Color(.systemBackground) instead of gradient backgrounds and decorative orbs.

HomeFeedView must not use .listStyle(.plain), .scrollContentBackground(.hidden), .background(ChirpColor.bg), or custom toolbar button styling.

ProfileView must not use banner gradients; follow/unfollow buttons must be placed in .toolbar.

SearchView must use plain TextField and Button with Label instead of capsule buttons, GlassCard, and custom input backgrounds.

NotificationsView must use plain Image, Text, and LazyVGrid instead of custom glow rings, capsule badges, and GlassCard. [^5d893-8]

## nmp-gallery Registry Adoption

Chirp iOS must adopt the `nmp-gallery` registry components (NostrAvatar, NostrProfileName, NostrProfileHost) instead of its current static inline implementations. The NostrProfileHost environment must be injected at the Chirp app root via `.environment(\.nostrProfileHost, model)` in `ChirpApp.swift`. [^9a2c7-6]
## See Also

