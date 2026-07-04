---
title: "Fabric Snapshot: Agent Ambient Awareness"
slug: fabric-snapshot
topic: agent-coordination
summary: The fabric snapshot is a hook-provided ambient awareness block that tells an agent its identity, current channel, nearby agents, recent changes, and invitable a
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-03
updated: 2026-07-03
verified: 2026-07-03
compiled-from: conversation
sources:
  - session:1c293d33-5ec2-4689-b6c2-cd159d8b6bb7
---

# Fabric Snapshot: Agent Ambient Awareness

## Fabric Snapshot

The fabric snapshot is a hook-provided ambient awareness block that tells an agent its identity, current channel, nearby agents, recent changes, and invitable agents. The `tenex-edge who` command refreshes agent awareness and should be used only when the injected fabric snapshot is unavailable, stale, or lost after context compression. <!-- [^1c293-e3c6c] -->
