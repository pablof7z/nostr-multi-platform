---
title: Chirp Session Storage
slug: chirp-session-storage
summary: chirp-tui and chirp-desktop store the session key in a simple file on disk rather than in the system keychain.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d5f3f755-8e68-47e1-86d3-29037ef9ddb8
  - session:34d8cff3-a7d4-4b49-a912-d2f465f53a29
---

# Chirp Session Storage

## Session Storage Location

chirp-tui and chirp-desktop store the session key in a simple file on disk rather than in the system keychain. TUI chirp-repl fails at mls-init with errSecMissingEntitlement (-34018) because unsigned binaries cannot access the macOS Keychain. Existing users must perform one re-login after the migration to file-based session storage.

<!-- citations: [^d5f3f-1] [^d5f3f-2] [^34d8c-1] -->
## Implementation Reference

chirp-tui uses the chirp-desktop file-based session implementation as the reference for its own implementation. <!-- [^d5f3f-3] -->
