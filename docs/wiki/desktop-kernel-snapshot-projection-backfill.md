---
title: Desktop KernelSnapshot Projection Backfill — Fields Are in projections, Not Top-Level
slug: desktop-kernel-snapshot-projection-backfill
summary: After V-80, chirp-desktop reads active_account, profile, accounts, and items from empty struct defaults — these must be backfilled from the kernel's projections map.
tags:
  - capture
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:ecf13381-c8ef-40bf-9498-04a1d1f2af8f
---

# Desktop KernelSnapshot Projection Backfill — Fields Are in projections, Not Top-Level

> After V-80, chirp-desktop reads active_account, profile, accounts, and items from empty struct defaults — these must be backfilled from the kernel's projections map.

## Root Cause

After V-80 (OP-feed migration), KernelSnapshot has no top-level items, profile, active_account, or accounts fields. All of these are now in the projections map. The desktop app reads them from the struct fields, which are always the empty default, causing the timeline to render Connecting to relays even after successful sign-in and event receipt. [^ecf13-17]

## Affected Fields

The following fields that chirp-desktop reads from KernelSnapshot top-level struct fields are always empty defaults: active_account, profile, accounts, and items. All of these live in projections after V-80. The make_update function confirms they must be backfilled from the projections map after deserialization. [^ecf13-18]

## Required Fix

After deserializing the kernel snapshot, the desktop app must backfill active_account, profile, accounts, and items from the kernel's projections map. Without this backfill, the identity panel and timeline are permanently empty regardless of account state. [^ecf13-19]


This fix landed in PR #796 alongside the session storage change. The projection backfill was implemented in app.rs, backfilling active_account, profile, accounts, and items right after deserialization. The build was chained immediately after the edit to prevent the linter from reverting the change. [^ecf13-38]
## Symptom

The desktop app receives events (visible in diagnostics) but renders zero notes. The timeline always shows Connecting to relays. This is because timeline() renders from snap.items which is always empty — the notes are in projections[nmp.feed.home] via the FlatBuffers typed sidecar. [^ecf13-20]


The chirp-desktop linter automatically reverts edits to app.rs if the build is not run immediately after the edit. To persist projection backfill changes in app.rs, the edit and cargo build must be chained atomically in a single shell invocation — any gap allows the linter to revert the change. [^ecf13-39]
## See Also
- [[chirp-desktop-feature-parity|Chirp Desktop Feature Parity — What Landed and Remaining Gaps]] — related guide
- [[op-centric-home-feed|OP-Centric Home Feed (V-80) — Architecture and Status]] — related guide

