---
title: Chirp UI Animations and Haptic Feedback
slug: chirp-animation-and-haptics
summary: The like button uses a spring animation with response 0.25 and dampingFraction 0.4 on tap
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-21
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
---

# Chirp UI Animations and Haptic Feedback

## Like Button

The like button uses a spring animation with response 0.25 and dampingFraction 0.4 on tap. Like buttons in ProfileNoteRow and ThreadNoteRow use the same spring animation and haptic feedback as the main feed. [^19e07-1]


## Compose Character Counter

The compose character counter is a circular progress ring that turns orange at ≤20 chars remaining and red when over the limit. [^19e07-2]

## Image Loading

Note images fade in on load using a FadeInModifier instead of appearing abruptly. [^19e07-3]

## Onboarding

The onboarding welcome screen has a staggered fade and slide-up entrance animation using the previously-unused appeared state. [^19e07-4]

## Haptic Feedback

Haptic feedback patterns in Chirp are consistent: soft for like, light for chat send and unfollow, medium for follow, success for publish. [^19e07-5]
## See Also

