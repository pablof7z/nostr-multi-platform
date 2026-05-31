---
title: FollowListProjection
slug: follow-list-projection
summary: "A FollowListProjection observes kind:3 events, maintains follows keyed by author, and exposes a snapshot with all display fields pre-computed for use by the con"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-27
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:eb342a0d-84e3-4289-9873-88a947ca8144
  - session:ad738906-eb99-4e37-9894-c5637626e6b0
---

# FollowListProjection

## Overview

A FollowListProjection observes kind:3 events, maintains follows keyed by author, and exposes a snapshot with all display fields pre-computed for use by the contact picker. The follow-feed REQ backfill limit per followed pubkey is 1000 events.

<!-- citations: [^eb342-3] [^ad738-1] -->
## See Also

