---
title: Onboarding Navigation Bug
slug: onboarding-navigation-bug
topic: ui-components
summary: The welcome screen renders inline when features.accounts is empty, showing 'chirp' title, 'the nostr social client' subtitle, and key hints
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-26
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:27a9cbf3-1348-44f6-bc0f-95a0a9c6ad84
  - session:c4b2e655-ca6b-42d2-9383-89bf52215d0a
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:0048057e-cb95-4da0-9f74-039a07dfc89f
---

# Onboarding Navigation Bug

## Onboarding Navigation Bug

The welcome screen renders inline when features.accounts is empty, showing 'chirp' title, 'the nostr social client' subtitle, and key hints. The current git branch, commit hash, and build time are displayed at the bottom of the welcome screen, picked up automatically from git and the build system without manual updates. This build info footer is pinned via .safeAreaInset(edge: .bottom) rather than as a VStack child, to avoid a SwiftUI layout crash on the iOS 26 beta. Tapping "Create a new identity" or any other login type on a physical iPhone does nothing — the user remains on the previous screen. RootShell transitions from OnboardingView to mainTabs based on model.hasActiveAccount.

<!-- citations: [^27a9c-7] [^c4b2e-5] [^93c59-1] [^00480-3] -->
