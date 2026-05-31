---
title: Chirp iOS Repost (NIP-18) — Implementation and Wiring
slug: chirp-ios-repost-nip18
summary: "Repost (NIP-18 kind:6) is implemented in Chirp iOS via PublishRaw dispatch, wired through all views (home feed, thread, profile), delivered in PR #811."
tags:
  - chirp-ios
  - repost
  - nip-18
  - social-actions
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-21
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:17ef19cd-8549-4fa9-b09c-5266aaf480a7
---

# Chirp iOS Repost (NIP-18) — Implementation and Wiring

> Repost (NIP-18 kind:6) is implemented in Chirp iOS via PublishRaw dispatch, wired through all views (home feed, thread, profile), delivered in PR #811.

## Overview

Repost (NIP-18 kind:6) enables users to repost notes in their home feed. The feature was implemented as a pure Swift + Rust dispatch path — the Rust kernel already supports kind:6 publishing via the existing `dispatch_action` seam using `PublishRaw { kind: 6, tags: [["e", eventId], ["p", pubkey]], content: "" }`. The gap was purely in the Swift UI layer: no `repost` bridge method, no `onRepost` closure, and the `NoteActionsRow` repost button was a no-op comment. [^4edd4-20]


Reposts (kind:6 events) render their inner note's content with a 'Repost' badge instead of showing raw JSON or the wrapper event. [^17ef1-1]

Tapping a kind:6 repost navigates to the inner note's thread rather than the wrapper event's thread. [^17ef1-2]
## Pre-Implementation State

Before PR #811, the repost button in `NoteActionsRow` was explicitly a no-op with the comment `// Repost command not yet on kernel surface`. The Rust-side dispatch path (`PublishRaw { kind: 6 }`) already existed and was functional — the gap was only in the Swift UI wiring. The compose button was investigated during the same testing pass and was found to be correctly wired (`showCompose = true`); the Haiku agent had tapped the paperplane (outbox) icon by mistake. [^4edd4-21]


The previous `effectiveContent` heuristic for detecting reposts checked for the `sig` field, which failed when relays stripped it from kind:6 events, causing raw JSON to display. [^17ef1-4]
## Implementation Layers

The repost feature was wired through four layers: (1) `KernelBridge.repost(eventId:pubkey:)` — builds the `PublishRaw` JSON envelope with kind:6 and the `e`/`p` tags, dispatches via the existing `dispatch_action` C FFI symbol; (2) `KernelModel.repost(eventId:pubkey:)` — thin wrapper calling `bridge.repost()`; (3) `NoteActionsRow` — gains an `onRepost: (() -> Void)?` closure parameter wired to the repost button's tap action; (4) Call sites in `HomeFeedView`, `ThreadScreen`, and `ProfileView` — each passes the `model.repost(eventId:pubkey:)` closure. [^4edd4-22]

## Wiring Through ModularBlockView and NoteRowView

The `onRepost` closure must be threaded through `ModularBlockView` → `NoteRowView` to reach individual note rows in the home feed. `ModularBlockView` accepts `onRepost` and passes it to `NoteRowView`. In `HomeFeedView`, `TimelineListView` passes `onRepost` through to `ModularBlockView`. For thread and profile views, `ThreadNoteRow` is used instead of `NoteRowView` — it also needs an `onRepost` parameter, wired at both call sites (`ThreadScreen` and `ProfileView`). Swift requires argument order to match declaration: `onRepost` comes after `onReply` in all signatures. [^4edd4-23]


ModularBlockView propagates `kind` through `syntheticItem` so reposts are correctly identified in that view. [^17ef1-3]
## Dispatch Envelope

The repost dispatch uses the existing `PublishRaw` action envelope: `{"PublishRaw": {"kind": 6, "tags": [["e", eventId], ["p", pubkey]], "content": ""}}`. This follows NIP-18: kind 6 is the standard repost event kind; the `e` tag references the note being reposted and the `p` tag references its author. No new FFI symbols are required — this uses the generic `dispatch_action` seam that already exists. [^4edd4-24]

## PR and Validation

PR #811 delivered the repost feature. The PR agent noted there was unrelated profile-lookup drift in `KernelModel.swift` not yet in a PR — this is tracked separately. The feature was validated as part of the cross-platform testing battery where a Haiku agent identified repost as a non-functional button during iOS social-actions testing. [^4edd4-25]

## See Also
- [[chirp-cross-platform-feature-parity-testing|Chirp Cross-Platform Feature Parity — Mandated Testing Across All Clients]] — related guide
- [[chirp-ios-embed-system-implementation|Chirp iOS Embed System — Implementation and Architecture]] — related guide
- [[android-write-capability|Android Write Capability — Dispatch Door and Write Baseline]] — related guide

