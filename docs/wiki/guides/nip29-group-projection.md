---
title: NIP-29 Group Projection
slug: nip29-group-projection
topic: marmot
summary: The NIP-29 group projection (`GroupTimelineProjection`) manages a timeline of events for group chats
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-26
updated: 2026-06-26
verified: 2026-06-26
compiled-from: conversation
sources:
  - session:ccf39f42-1717-41d2-aa85-48f6d27e6298
---

# NIP-29 Group Projection

## Description

The NIP-29 group projection (`GroupTimelineProjection`) manages a timeline of events for group chats. The projection is implemented in the NMP (NostrMP) layer with the namespace key `nmp.nip29.group_timeline`. <!-- [^ccf39-4f189] -->

## Types

Primary types:
- `GroupTimelineProjection`: the projection class
- `GroupTimelineEvent`: individual timeline event
- `GroupTimelineSnapshot`: projection state snapshot <!-- [^ccf39-0eb6f] -->

## Data Structure

The projection exposes an `events` field containing the timeline of group events. <!-- [^ccf39-b0a1b] -->

## FFI and Serialization

FFI C-ABI symbols are named according to the timeline pattern (e.g., `nmp_app_chirp_register_group_timeline`). The FlatBuffers file identifier is `NGTL`. <!-- [^ccf39-4d609] -->

## Implementation Notes

The projection is NMP-layer only. Chirp's UI components (`GroupChatView`, `GroupChatStore`, `GroupChatMessageRow`) remain unchanged. Android has no references to this projection and requires no changes. <!-- [^ccf39-5b946] -->
