---
title: Chirp iOS Secure Input Fields — Manual Paste and Privacy Design
slug: chirp-ios-secure-input-fields
summary: The nsec input field is a plain SecureField with no automatic clipboard reading, allowing users to paste manually via long-press
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-19
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:ae3081b4-af7d-4117-a25f-c05a78479b35
  - session:5d893073-9635-450b-b8e9-50648bc1a4e7
---

# Chirp iOS Secure Input Fields — Manual Paste and Privacy Design

## Secure Input Fields

The nsec input field is a plain SecureField with no automatic clipboard reading, allowing users to paste manually via long-press. The bunker:// input field likewise has no automatic clipboard reading, allowing users to paste manually via long-press. [^ae308-1]



Users can still paste via the standard iOS text field long-press menu after clipboard paste buttons are removed. [^5d893-10]
## Onboarding Paste Privacy

Onboarding views must not read UIPasteboard.general.string inside SwiftUI body, as it triggers repeated clipboard access prompts on every app launch. (Previously: OnboardingView.swift does not read UIPasteboard.general.string on render, preventing the iOS paste privacy banner from appearing during onboarding.)

<!-- citations: [^ae308-2] [^5d893-9] -->
## See Also

