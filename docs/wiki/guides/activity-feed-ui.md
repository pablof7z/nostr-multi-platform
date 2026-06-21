---
title: Activity Feed UI
slug: activity-feed-ui
topic: ui-components
summary: Activity is accessed via a bell toolbar button in the top-right of HomeFeedView, positioned left of the compose button
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-21
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:cb3376a7-cea1-49ac-b6dd-9251fa1af14a
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
---

# Activity Feed UI

## Access & Navigation

Activity is accessed via a bell toolbar button in the top-right of HomeFeedView, positioned left of the compose button. It is not a tab on the bottom bar. Tapping the bell toolbar button presents NotificationsView in a NavigationStack sheet. <!-- [^cb337-1] -->

AccountSummary includes a pictureUrl field enriched from the profile cache via accounts_enriched(), so the user's own avatar appears in the home feed toolbar and compose sheet. <!-- [^19e07-1] -->
