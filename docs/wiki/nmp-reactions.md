---
title: "NMP Reactions Crate: NIP-25 & NIP-18"
slug: nmp-reactions
summary: The `nmp-reactions` crate combines NIP-25 (kind 7) and NIP-18 (kind 6/16) under a `SocialRecord` tagged enum with `ReactionsDomain`, `ReactionSummaryView`, and
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

# NMP Reactions Crate: NIP-25 & NIP-18

## Overview

The `nmp-reactions` crate combines NIP-25 (kind 7) and NIP-18 (kind 6/16) under a `SocialRecord` tagged enum with `ReactionsDomain`, `ReactionSummaryView`, and `RepostsView`. Reaction writes bypass the crate's builder, occurring directly at publish.rs:114-119.

<!-- citations: [^590ca-8] [^57528-15] -->
## See Also

