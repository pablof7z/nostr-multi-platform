---
title: PR #823 — TimelineItem author_display_name Structural Flicker Fix
slug: timeline-item-author-display-name-structural-fix-pr-823
summary: PR #823 adds author_display_name to Rust TimelineItem at snapshot build time, making the display name independent of claim lifecycle and structurally preventing the profile name flicker.
tags:
  - ios
  - rust
  - flicker
  - pr-823
  - timeline-item
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-21
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:17ef19cd-8549-4fa9-b09c-5266aaf480a7
---

# PR #823 — TimelineItem author_display_name Structural Flicker Fix

> PR #823 adds author_display_name to Rust TimelineItem at snapshot build time, making the display name independent of claim lifecycle and structurally preventing the profile name flicker.

## Overview

PR #823 is the structural fix for the profile name flicker. Instead of relying on the claim lifecycle to deliver display names (which creates a 1–2 tick gap on navigation), the display name is baked directly into every TimelineItem at Rust snapshot-build time from the resident profile cache. This makes the name available in the FlatBuffers snapshot independent of whether the profile is currently claimed, eliminating the flicker at the architectural level. [^4edd4-82]

## What Was Added to Rust

The TimelineItem struct in Rust gained an author_display_name: Option<String> field and a kind: u32 field. The kind field distinguishes kind:6 reposts from kind:1 notes without relying on fragile JSON heuristic detection; it is exported to Swift as UInt32. The author_display_name field is populated at snapshot-build time from the kernel's resident profile cache — the same cache that claim_profile reads from with zero relay REQ for already-resident pubkeys. This field is exported to codegen so it appears in the generated Swift KernelTypes.generated.swift as item.authorDisplayName.

<!-- citations: [^4edd4-83] [^17ef1-5] -->
## What Was Changed in Swift

NoteRowView.authorDisplayLabel and ThreadNoteRow were updated to use item.authorDisplayName as a claim-independent fallback in the display name resolution chain. The updated fallback priority is: claimedProfiles → mentionProfiles → item.authorDisplayName (new, from FlatBuffers snapshot) → pubkey.shortHex. This means that even when a profile is released (removing it from claimedProfiles), the display name persists in the snapshot item itself, preventing the flicker to hex. [^4edd4-84]

## Why This Fixes the Flicker Structurally

Before PR #823, the display name was only available through the claim lifecycle: claim_profile → claimed_profiles projection → UI. When a component released its claim on navigation, the name vanished from the projection, and the next snapshot tick dropped it — creating the 1–2 tick flicker window. After PR #823, the display name is embedded in the snapshot item itself, independent of claims. The kernel populates author_display_name from the resident profile cache at snapshot-build time, and the cache persists regardless of claim state. The name is always present in the snapshot as long as the kind:0 metadata has ever been fetched. [^4edd4-85]

## Swift Codegen Learning

The Swift codegen tool uses the flag --schemas - (not --stdin). This was confirmed during the PR #823 codegen regeneration step.

<!-- citations: [^4edd4-86] [^4edd4-122] [^4edd4-234] -->
## iOS Sim Build Learning

Building for the iOS simulator requires `just rust-ios-sim`, not bare `cargo build -p nmp-app-chirp`. The just command wraps the correct target (aarch64-apple-ios-sim) and feature flags (including --features marmot).

<!-- citations: [^4edd4-87] [^4edd4-123] [^4edd4-235] -->
## Merge Order Dependency

PR #823 depends on PR #821 (warm-reclaim Rust tests) being merged first for a clean master tip. After #823 lands, PRs #822 (UI tests) and #824 (instrumentation) must be rebased onto the new master because #823 changes NoteRowView.swift — the same file they touch. [^4edd4-88]

## See Also

