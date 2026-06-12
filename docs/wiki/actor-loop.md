---
title: Actor Loop
slug: actor-loop
topic: actor-loop
summary: The actor loop uses a dual-channel design with COMMAND_DRAIN_BUDGET for commands and recv_timeout for relay events, preventing command starvation during relay e
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf
---

# Actor Loop

## Dual-Channel Design

The 4Hz snapshot transport uses a dual-priority channel where commands drain with a burst budget at each iteration and relay events use recv_timeout, preventing command starvation during relay event floods. kernel/mod.rs, actor/dispatch.rs, and actor/mod.rs are 3-5x over the 500-LOC hard ceiling specified in AGENTS.md. React, Follow, and Unfollow remain as deprecated ActorCommand variants in nmp-core while nmp-nip02 ActionModules handle the real dispatch.

<!-- citations: [^da6b1-41] [^da6b1-42] [^da6b1-1] [^b4fe9-1] [^da6b1-40] [^da6b1-95] -->
