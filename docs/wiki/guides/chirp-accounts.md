---
title: Chirp Account Flows and Default Follows
slug: chirp-accounts
topic: app-accounts
summary: "Chirp account-creation (`nmp_app_chirp_create_new_account` / `ChirpApp::create_new_account`) publishes its own kind:3 contact list via `chirp_default_follows()`"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp Account Flows and Default Follows

## Default Follows Behavior

Chirp account-creation (`nmp_app_chirp_create_new_account` / `ChirpApp::create_new_account`) publishes its own kind:3 contact list via `chirp_default_follows()` at `created_at≈now`, which supersedes any older seeded kind:3. Sign-in via nsec import (`signin_nsec` / `nmp_app_signin_nsec`) takes the plain `add_signer` path and must not trigger this default-follows publication.

<!-- citations: [^dcc80-379bc] [^dcc80-0b881] [^dcc80-47223] -->
## Feed Scope Composition by Shell

The iOS home feed uses plain `ActiveUserFollows` scope built in Swift, not the `Difference(follows, mute)` composition used by the Rust Android/desktop/TUI shells. The `Difference(follows, mute_list)` composition hard-errors pre-login with `ScopeNotSupportedYet` because the mute source requires an active account, whereas plain follows degrades gracefully to empty. <!-- [^dcc80-2eb0c] -->
