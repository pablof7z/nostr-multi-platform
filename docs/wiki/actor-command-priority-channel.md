---
title: Actor Command Priority Channel — Preventing Dropped Commands
slug: actor-command-priority-channel
summary: "Onboarding sign-in failed because ActorCommand::CreateAccount and SignInNsec were silently dropped by try_send when the 4096-slot bounded channel was full of re"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-21
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
---

# Actor Command Priority Channel — Preventing Dropped Commands

## Root Cause of Sign-In Failure

Onboarding sign-in failed because ActorCommand::CreateAccount and SignInNsec were silently dropped by try_send when the 4096-slot bounded channel was full of relay events during startup. [^582fc-1]


## Priority Channel Architecture

A dual-channel priority architecture resolves dropped commands: command_rx is polled via try_recv at the top of every actor loop iteration before relay events are processed via recv_timeout. This approach gives commands near-zero latency regardless of relay event volume, because try_recv drains all pending commands before each recv_timeout on the relay channel. Note that the relay-event channel (relay_tx) in actor/mod.rs is unbounded, allowing potential unbounded accumulation during a flood. The microblog walkthrough (§19a) must use PublishNote (the kind:1-specific variant) rather than a generic PublishUnsignedEvent when dispatching ActorCommand. The command drain must not be wrapped in catch_unwind; internally-generated commands should panic-loud on failure.

<!-- citations: [^582fc-2] [^09da8-1] [^c3f75-3] [^1c093-5] -->
## See Also

