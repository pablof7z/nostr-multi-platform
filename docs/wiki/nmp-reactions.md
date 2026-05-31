---
title: nmp-reactions — Social Relations Facade (Reactions, Reposts, Comments, Zaps)
slug: nmp-reactions
summary: NMP provides applesauce-style ergonomic access to relations (e.g., 'give me the likes', 'build a reply event') as part of the library rather than leaving it as
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
---

# nmp-reactions — Social Relations Facade (Reactions, Reposts, Comments, Zaps)

## Ergonomic Relations API

NMP provides applesauce-style ergonomic access to relations (e.g., 'give me the likes', 'build a reply event') as part of the library rather than leaving it as app-layer boilerplate. NMP provides a Relations facade in nmp-reactions composing nip01/nip22/nip57 with entrypoints Relations::for_event(id, kind) → RelationSpecs, Relations::reply_to, react_to, repost, zap_request, comment_on as pure free-function composition with no store reference. Reaction writes must go through the nmp-reactions builder instead of bypassing it.

<!-- citations: [^590ca-8] [^57528-22] -->
## Crate Scope

nmp-reactions is a combined crate for NIP-25 (kind 7 reactions) and NIP-18 (kind 6/16 reposts) under a SocialRecord tagged enum, replacing the originally planned split into separate nmp-nip25 and nmp-nip18 crates. [^590ca-9]
## See Also

