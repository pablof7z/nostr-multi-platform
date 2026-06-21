---
title: Profile Header UI
slug: profile-header-ui
topic: ui-components
summary: The profile banner header uses a LinearGradient derived from the user's avatarColor at 28% opacity fading to secondarySystemBackground.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-19
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:e6b44a84-8cfc-48b2-863a-58382398b5df
---

# Profile Header UI

## Profile Banner Header

The profile banner header uses a LinearGradient derived from the user's avatarColor at 28% opacity fading to secondarySystemBackground. <!-- [^19e07-7] -->

## Profile Data Gating

Profile screens gate the about/bio and nip05 display behind hasProfile == true to avoid showing debug text like 'Waiting for selected author kind:0'. ProfileCard includes a has_profile boolean field populated from profile.is_some() in Rust. <!-- [^19e07-8] -->

## Follow Button State

Navigating to the profile of a person the user is already following must display the follow button in a state that indicates the existing follow relationship (was incorrectly showing "Follow"). Tapping the follow button on a profile that the user does not already follow must update the button's visual state to reflect the new follow relationship (was staying as "follow" with no visual change). <!-- [^e6b44-2] -->
