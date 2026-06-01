---
title: NMP Action Stage Lifecycle & UI Dispatch
slug: nmp-action-stage-lifecycle
summary: The DM-inbox publish UI is driven from the action_results terminal stage instead of optimistic updates
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-06-01
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:594b7c34-efd1-4461-81ad-9fa33a6e76f9
  - session:89070aba-0e77-4da3-99e1-322addb1c747
---

# NMP Action Stage Lifecycle & UI Dispatch

## Action Stage Lifecycle

The DM-inbox publish UI is driven from the action_results terminal stage instead of optimistic updates. PublishNote (kind:1) is wired end-to-end on WASM through dispatch_app_action_async. React, Follow, and Unfollow app actions on WASM return publish_path_not_wired_for_kind, with only PublishNote currently wired. Zap payment feedback follows the same terminal-action observation pattern as DM-inbox publish, using recentTerminal(correlationId:) on KernelModel. The terminal stage of the stored correlation ID is observed via an .onChange(of: model.actionLifecycle) modifier.

<!-- citations: [^2c4ad-10] [^594b7-5] [^89070-2] -->
