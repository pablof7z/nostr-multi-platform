---
title: Action Stage Ack-Based Retention — Preventing Projection Races
slug: action-stage-ack-retention
summary: Action stage retention uses an ack-based contract where the host calls `nmp_app_ack_action_stage(correlation_id)` to drop terminal states, preventing one-tick p
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-22
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
---

# Action Stage Ack-Based Retention — Preventing Projection Races

## Action Stage Ack Retention

Action stage retention uses an ack-based contract where the host calls `nmp_app_ack_action_stage(correlation_id)` to drop terminal states, preventing one-tick projection races with the Swift drain. PD-036 requires choosing between adding ActorCommand::RecordActionAccepted or flipping is_async_completing to false, because the ZapAction success path never records Accepted ActionStage.

<!-- citations: [^1c093-4] [^95d02-2] -->
## See Also

