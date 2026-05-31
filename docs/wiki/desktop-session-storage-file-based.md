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
  - session:d5f3f755-8e68-47e1-86d3-29037ef9ddb8
---

# Desktop Session Storage Must Be File-Based — Not OS Keychain

> Chirp desktop must use file-based session storage matching chirp-tui's approach, not the OS keychain.

## Session Storage Requirement

Chirp-tui and chirp-desktop store session keys in a simple file rather than the system keychain. The OS keychain must not be used for desktop session persistence. The keychain path causes a system-level keychain access prompt on launch when a saved session exists, interrupting the user experience and blocking the app until the user responds.

Sessions are stored as plain files at chirp_data_dir()/sessions/<account_id>, with permissions set to chmod 0600. The keyring crate dependency is removed. The keyring_handler signature in bridge.rs is preserved — only the underlying storage mechanism changed. [^ecf13-31]

With file-based storage, nsec is stored as plaintext on disk (previously encrypted by the OS keychain). The file permissions (0600) are the sole protection. Existing users must perform one re-login after the migration to file-based session storage — there is no migration path from keychain-stored sessions. [^ecf13-32]

PR #796 (commit f63dcfda) landed the file-based storage: chirp-desktop/src/keyring.rs no longer hits the OS keychain. Sessions stored as plain files at chirp_data_dir()/sessions/<account_id>, chmod 0600. The keyring crate dependency was removed. No changes were needed in bridge.rs because the keyring_handler signature was preserved. Both TUI and desktop should eventually converge on file-based storage. [^ecf13-34]

<!-- citations: [^ecf13-31] [^ecf13-32] [^ecf13-34] [^ecf13-14] [^ecf13-29] [^d5f3f-1] -->
## Keychain Prompt Problem

When the desktop app uses OS keychain for session storage and a saved session is found on launch, macOS presents a system keychain access prompt. This blocks the app until the user responds, creating friction that file-based storage avoids entirely. [^ecf13-15]

## Reference Implementation

chirp-tui uses the chirp-desktop file-based session storage implementation as its reference model and does not depend on the keyring crate for session storage. Both TUI and desktop now use file-based storage rather than the system keychain.

<!-- citations: [^ecf13-16] [^ecf13-30] -->

<!-- citations: [^ecf13-16] [^ecf13-30] [^d5f3f-2] -->
## See Also
- [[chirp-desktop-feature-parity|Chirp Desktop Feature Parity — What Landed and Remaining Gaps]] — related guide
- [[chirp-ffi-boot-and-callback-lifetime|Chirp FFI Boot Sequence & Callback Object Lifetimes]] — related guide

