---
title: Lifecycle Reset Desync Bug (#2932)
slug: lifecycle-reset-desync
topic: app-lifecycle
summary: "NMP#2932 is a latent desync bug where LifecycleCommand::Reset rebuilds the kernel with an empty active_account slot and nulls the shared slot, but does not touc"
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

# Lifecycle Reset Desync Bug (#2932)

## Bug Summary

NMP#2932 is a latent desync bug where LifecycleCommand::Reset rebuilds the kernel with an empty active_account slot and nulls the shared slot, but does not touch IdentityRuntime, so the following Start's restore_active_session early-returns and never re-syncs the account into the rebuilt kernel. The same bug manifests on the Chirp side via the reset+start trigger: the active account silently reverts to None, emptying the home feed until the user re-signs in.

<!-- citations: [^dcc80-89aab] [^dcc80-ea0c6] [^dcc80-32aeb] -->
## Active-Account Clearing Paths

NMP has no autonomous, async, or timer path that clears the active account; the active-account slot only goes None via an explicit host command: Reset, RemoveAccount, or AddSigner(AppManagedLocalNsec) of the currently-active pubkey.

<!-- citations: [^dcc80-3f5d5] -->
## Recommended Fix

LifecycleCommand::Reset must not leave IdentityRuntime::active set while the kernel's active-account slot is cleared, and restore_active_session must re-sync the account into a rebuilt kernel rather than early-returning on a stale identity state. Concretely: do not gate restore_active_session solely on identity.active_pubkey().is_some() after a kernel rebuild; instead either re-set_accounts from the surviving identity or reset identity in reset().

<!-- citations: [^dcc80-2598a] [^dcc80-b8505] -->
