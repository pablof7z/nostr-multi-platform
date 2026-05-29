---
title: Desktop Session Storage Must Be File-Based — Not OS Keychain
slug: desktop-session-storage-file-based
summary: Chirp desktop must use file-based session storage matching chirp-tui's approach, not the OS keychain.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:ecf13381-c8ef-40bf-9498-04a1d1f2af8f
---

# Desktop Session Storage Must Be File-Based — Not OS Keychain

> Chirp desktop must use file-based session storage matching chirp-tui's approach, not the OS keychain.

## Session Storage Requirement

Chirp desktop must use file-based session storage. The OS keychain must not be used for desktop session persistence. The keychain path causes a system-level keychain access prompt on launch when a saved session exists, interrupting the user experience and blocking the app until the user responds.

Sessions are stored as plain files at chirp_data_dir()/sessions/<account_id>, with permissions set to chmod 0600. The keyring crate dependency is removed. The keyring_handler signature in bridge.rs is preserved — only the underlying storage mechanism changed. [^ecf13-31]

With file-based storage, nsec is stored as plaintext on disk (previously encrypted by the OS keychain). The file permissions (0600) are the sole protection. There is no migration path from keychain-stored sessions — existing sessions require one re-login after the switch. [^ecf13-32]

PR #796 (commit f63dcfda) landed the file-based storage: chirp-desktop/src/keyring.rs no longer hits the OS keychain. Sessions stored as plain files at chirp_data_dir()/sessions/<account_id>, chmod 0600. The keyring crate dependency was removed. No changes were needed in bridge.rs because the keyring_handler signature was preserved. Both TUI and desktop should eventually converge on file-based storage. [^ecf13-34]

<!-- citations: [^ecf13-14] [^ecf13-29] -->
## Keychain Prompt Problem

When the desktop app uses OS keychain for session storage and a saved session is found on launch, macOS presents a system keychain access prompt. This blocks the app until the user responds, creating friction that file-based storage avoids entirely. [^ecf13-15]

## Reference Implementation

chirp-tui actually also uses the OS keychain (same pattern as the old desktop code) — there is no file-based reference to copy. The file-based storage for desktop was implemented from scratch, independent of the TUI approach. Both TUI and desktop should eventually converge on file-based storage.

<!-- citations: [^ecf13-16] [^ecf13-30] -->
## See Also
- [[chirp-desktop-feature-parity|Chirp Desktop Feature Parity — What Landed and Remaining Gaps]] — related guide
- [[chirp-ffi-boot-and-callback-lifetime|Chirp FFI Boot Sequence & Callback Object Lifetimes]] — related guide

