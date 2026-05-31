---
title: Chirp Session Storage
slug: chirp-session-storage
summary: chirp-tui and chirp-desktop store the session key in a simple file on disk rather than in the system keychain.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d5f3f755-8e68-47e1-86d3-29037ef9ddb8
---

# Chirp Session Storage

## Session Storage Location

chirp-tui and chirp-desktop store the session key in a simple file on disk rather than in the system keychain. [^d5f3f-1]


Existing users must perform one re-login after the migration to file-based session storage. [^d5f3f-2]

## Implementation Reference

chirp-tui uses the chirp-desktop file-based session implementation as the reference for its own implementation. [^d5f3f-3]
## See Also

