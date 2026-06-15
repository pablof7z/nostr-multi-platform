---
title: Chirp iOS Visual Style
slug: chirp-ios-visual-style
topic: ui-components
summary: Chirp iOS uses a minimalistic, monochrome visual palette anchored on a single black accent (Vercel-style), replacing the previous scattered blue/cyan/yellow/red
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-15
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:286c6f24-af4b-4e59-b72f-ed72e8b9d781
  - session:c9a794f6-6ad7-4ee9-a620-fc342fd495c3
---

# Chirp iOS Visual Style

## Visual Direction & Completion

Chirp iOS uses a minimalistic, monochrome visual palette anchored on a single black accent (Vercel-style), replacing the previous scattered blue/cyan/yellow/red colors. Polish progress is measured by iterating until a codex exec review confirms the app is 'amazing' (achieved a 9/10 'Ship it. The monochrome direction is coherent.' rating). <!-- [^286c6-1] -->

## Color System

The app's global accent color is set via an adaptive black/white AccentColor asset in the asset catalog combined with a global-accent build setting in project.yml, because .tint() alone does not recolor Form row icons. ChirpColor.accent and ChirpColor.link are adaptive black/white, and ChirpColor.onAccent is set for contrast on the now-black bubble foreground. <!-- [^286c6-2] -->

## Components & Patterns

Avatar fallbacks use a monochrome grayscale gradient instead of a colored gradient, with calmer, smaller initials. NostrAvatar conforms to Equatable on all rendered inputs (pubkey, url, colorHex, initials, size) so SwiftUI can skip body re-evaluation when profile data is unchanged; the conformance must be activated by applying .equatable() or wrapping with EquatableView at call sites, because bare Equatable conformance on a SwiftUI View is not automatically consulted by the diffing engine. Late picture arrival must be verified to still repaint NostrAvatar when url == nil (host-backed) after adding Equatable, since a profile arrival that doesn't change any input field may not trigger re-evaluation. Primary CTAs use a rounded-rect shape with a corner radius of 12-14 points, not a full capsule/pill shape. Placeholder/empty-state icons use a secondary/tertiary tone instead of full black/primary.

<!-- citations: [^286c6-3] [^c9a79-12] [^c9a79-18] -->
## Screen-Specific Styling

The Settings screen uses a native inset-grouped Form with consistent native Section headers, compact list section spacing, and no custom background overrides. The Wallet screen removes the candy-colored 'Powered By' footer tiles and uses a monochrome hero with a de-pilled black CTA. The Profile screen uses a shorter flat neutral banner, a native left-aligned 'Posts' header without a decorative capsule, and an 'Edit Profile' button styled as a quiet secondary gray chip instead of a solid black primary button. In the Thread view, the focused note no longer displays a bright-blue tinted card with a thick accent rail; instead, it uses the standard 0.5pt separator, and its action bar matches the feed's canonical NoteActionsRow (reply/repost/like/zap). The Compose screen hides the progress ring when the character count is 0, showing it only once the user starts typing. The embedded-event loading state displays a skeleton placeholder instead of raw 'nostr:nevent1q…' text. <!-- [^286c6-4] -->
