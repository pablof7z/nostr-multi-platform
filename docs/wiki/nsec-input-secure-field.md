---
title: Nsec Input Secure Field & Manual Paste
slug: nsec-input-secure-field
summary: The nsec input field is a plain SecureField with no automatic clipboard-reading Paste button, allowing users to paste manually via long-press → Paste
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

# Nsec Input Secure Field & Manual Paste

## Nsec Input Field

The nsec input field is a plain SecureField with no automatic clipboard-reading Paste button, allowing users to paste manually via long-press → Paste. Onboarding views must not read UIPasteboard.general.string inside the SwiftUI body, as that triggers repeated clipboard access prompts on every app launch. The clipboard paste affordance block has been removed from OnboardingView+Components.swift.

<!-- citations: [^ae308-1] [^5d893-11] -->
## Bunker URI Input Field

The bunker:// input field has no automatic clipboard-reading Paste button, allowing users to paste manually via long-press → Paste. The clipboard paste affordance block has been removed from OnboardingView+NIP46.swift.

<!-- citations: [^ae308-2] [^5d893-12] -->
## See Also

