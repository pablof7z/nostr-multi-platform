---
title: UI Animations and Haptics
slug: ui-animations-and-haptics
topic: ui-components
summary: Tapping the like button triggers a spring animation that scales the heart icon to 135% with response 0.25 and dampingFraction 0.4
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

# UI Animations and Haptics

## Like Button

Tapping the like button triggers a spring animation that scales the heart icon to 135% with response 0.25 and dampingFraction 0.4. Like buttons in ProfileNoteRow and ThreadNoteRow include the same spring animation and UIImpactFeedbackGenerator(style: .soft) haptic feedback, matching the main feed NoteActionsRow. <!-- [^19e07-11] -->

## Chat Scrolling

DM conversations auto-scroll to the newest message on both initial load (with a 0.15s delay) and when new messages arrive via onChange(of: messages.count). Marmot group chat opens pinned to the latest message on appear, matching the DM conversation behavior. <!-- [^19e07-12] -->

## Image Fade-In

In-note images fade in on load using a FadeInModifier with 0.3s easeInOut animation. ChirpAvatar profile pictures fade in on load using a FadingImage helper with 0.2s easeInOut animation. <!-- [^19e07-13] -->

## Onboarding

The onboarding welcome screen has a staggered fade+slide-up entrance animation using the previously-unused `appeared` state variable. <!-- [^19e07-14] -->

## Action Haptics

Chat send actions trigger haptic feedback: light impact for DM and group chat sends, success notification for note publish, medium impact for follow, and light impact for unfollow. <!-- [^19e07-15] -->
