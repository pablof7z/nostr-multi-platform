---
title: Profile Color Assignment — Stable djb2 Hash from npub
slug: profile-color-assignment
summary: Profile colors are derived by djb2-hashing the npub (not the mutable display_name) for stable per-identity color assignment.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-22
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
---

# Profile Color Assignment — Stable djb2 Hash from npub

## Profile Color Assignment

Profile colors are derived by djb2-hashing the npub (not the mutable display_name) for stable per-identity color assignment. DefaultHasher is used in stable identifiers (contacts.rs, sub_key.rs, profile/thread request IDs) and is non-deterministic across processes, requiring replacement with a stable hash.

<!-- citations: [^4f377-13] [^95d02-17] -->
## See Also

