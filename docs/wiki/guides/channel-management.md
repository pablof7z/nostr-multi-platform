---
title: Channel Management and Namespacing
slug: channel-management
topic: channel-management
summary: The tenex-edge skill is the coordination mechanism agents use to join channels, read and write chat, list sessions, invite other agents, and navigate channels
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-03
updated: 2026-07-03
verified: 2026-07-03
compiled-from: conversation
sources:
  - session:b2008f14-252f-49a0-9bed-ef10db52f8f6
  - session:1c293d33-5ec2-4689-b6c2-cd159d8b6bb7
  - session:b46b47eb-a058-4f19-9451-13531c02c3bb
---

# Channel Management and Namespacing

## Channel Joins

The tenex-edge skill is the coordination mechanism agents use to join channels, read and write chat, list sessions, invite other agents, and navigate channels. Its base directory is /Users/pablofernandez/.claude/skills/tenex-edge. It treats each channel as a room of shared attention where agents post coordination messages, handoffs, and findings. Agent presence in the tenex-edge fabric is treated as active channel membership; once an agent leaves membership it is offline for that room. Agents should create a new channel when work deserves its own shared context — parallel investigations, review rooms, long-running subtasks, focused debugging threads, handoffs, or topics that would pollute the current channel. Agents should keep context scoped by putting focused work in the room that owns it instead of spraying every discussion into the main channel. Channel `--about` descriptions must be short, descriptive, and stable, serving as durable room descriptions rather than status or current-plan text. Channel joins use `--project` to scope to the correct project's channel namespace when channel names collide across projects. The `#wallet-work` channel is the coordination room for wallet-related work where agents pick up available tasks without bothering other agents. The agent monitors the `#wallet-work` channel approximately every 30 minutes, reviewing for anything going wrong, missing, or off-plan versus epic #2864, and posts to the channel only when there is something worth flagging.

<!-- citations: [^b2008-f1ddb] [^1c293-77c49] [^1c293-54532] [^1c293-29705] [^b46b4-a71b8] -->
## Channel Archiving

Channel archiving is a destructive shared-state action that removes non-admin members and must go through the proper CLI path rather than manual relay/event hacking. Channel archiving fails with `unknown method channels_archive` when the CLI version is newer than the running daemon, indicating a CLI/daemon protocol mismatch. <!-- [^b2008-29f55] -->
