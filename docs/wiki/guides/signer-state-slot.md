---
title: Signer State Slot and Remote Signer Health Display
slug: signer-state-slot
topic: app-lifecycle
summary: The `signer_state` slot is a kernel-owned global slot with no per-account identity or keying
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

# Signer State Slot and Remote Signer Health Display

## Signer State Slot

The `signer_state` slot is a kernel-owned global slot with no per-account identity or keying. Because it is never scoped or cleared per active account, two remote-signer accounts can cross-contaminate each other's health display — a framework gap filed as NMP#2976.

<!-- citations: [^dcc80-9c480] [^dcc80-34ac3] -->
## AccountsView Health Rendering

The iOS signer-relay health section in `AccountsView` gates on both `model.signerState != nil` AND the active account's `signerIsRemote` field, so it only renders for accounts that actually have a remote signer. This guard is required because `signerState` is a kernel-owned global slot that is never scoped or cleared per active account.

<!-- citations: [^dcc80-ee36e] [^dcc80-4b0fb] [^dcc80-d50a4] -->
