---
title: Chirp FeedParams Shape and Compilation
slug: chirp-feed-params
topic: app-feed
summary: "Chirp's FeedParams JSON shape uses the following fields: `shape`, `source`, `order`, `key`, `item_projection`, `primary_kinds`, `admission`, and `window`"
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

# Chirp FeedParams Shape and Compilation

## FeedParams JSON Shape

Chirp's FeedParams JSON shape uses the following fields: `shape`, `source`, `order`, `key`, `item_projection`, `primary_kinds`, `admission`, and `window`. This replaces the old `render`/`acquisition`/`ranking`/`projection` shape.

The `order` value is `NewestByFeedPosition` (replacing the old `ChronologicalDesc`).

The `key` field is a bare string.

The shape includes the required `item_projection: "FeedRows"` field.

<!-- citations: [^dcc80-82377] -->
## Open Feed Path

The `NmpApp::open_feed(params)` path goes through `nmp_feed_session::compile_feed_params` → `FeedShape::RootIndexed` → `build_op_scope_session` — the PullFeedController + op-feed observer + snapshot-emission wiring the device's home feed uses. <!-- [^dcc80-2028d] -->

## Home Feed Composition by Platform

On Rust shells (Android, desktop, and TUI), Chirp's home feed composition uses `Difference(ActiveUserFollows, ListMembers(ACTIVE_MUTE_LIST))` with `RootIndexed`, `primary_kinds [1]`, and `admission All`.

On iOS, the home feed uses plain `ActiveUserFollows` (not the difference composition), built in Swift in `KernelBridge+FeedOperations.swift`. <!-- [^dcc80-3c951] -->

## Pre-Login Behavior

The `Difference(follows, mute_list)` composition hard-errors pre-login with `ScopeNotSupportedYet` because the mute source is `RequireActive`. In contrast, plain `ActiveUserFollows` degrades gracefully to empty via `AllowMissingActive`.

This is tracked as NMP#2930: the `Difference(follows, mute)` feed composition hard-errors before an active account exists, while plain follows opens fine and shows empty. <!-- [^dcc80-aa161] -->
