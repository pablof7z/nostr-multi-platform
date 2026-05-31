---
title: Chirp TUI Architecture & Rendering Stack
slug: chirp-tui-architecture
summary: The Chirp TUI uses ratatui + crossterm as its rendering stack
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-29
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:f8543716-09b7-4884-8952-da52f571962e
  - session:16ca6097-5734-4d49-8b9a-0a20d42a324b
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Chirp TUI Architecture & Rendering Stack

## Rendering Stack

The Chirp TUI uses ratatui + crossterm as its rendering stack. The build uses ratatui 0.30 with simple multiline text input. [^4f377-4]



The TUI must consume shared snapshot types from `nmp-app-chirp`. [^f3d8d-10]
## Image Display

The image display fallback chain follows the order Kitty → iTerm2 → Sixel → Unicode halfblocks, using ratatui-image with Picker::from_query_stdio(). Author avatars are rendered inline using ratatui-image with auto-detection of Kitty/iTerm2/Sixel protocols and halfblock fallback; StatefulProtocol/StatefulImage must be used instead of the stateless Protocol/Image path to avoid silently blank images on area changes. iTerm2 image rendering is detected via TERM_PROGRAM=iTerm.app rather than the escape-sequence probe that iTerm2 silently ignores.

<!-- citations: [^4f377-5] [^93c59-8] -->
## Layout and Navigation

The TUI provides a multi-pane timeline model supporting parallel feeds (home/mentions/DMs/NIP-29). The relay→feed→event detail hierarchy is displayed using ranger-style miller columns mapping to the Nostr hierarchy. [^4f377-6]


The TUI uses a master-detail split layout (Approach B) with a 38% post list on the left and a 62% detail pane on the right. The relay→feed→event detail hierarchy is displayed using ranger-style miller columns mapping to the Nostr hierarchy. All TUI mockups and the final design include thread/reply trees, DMs, relay health, and fake nostr data. [^93c59-7]
## Notification Ranking

An unread priority bar ranks notifications in the order: mentions/zaps > DMs > reactions > noise, following the weechat hotlist pattern. [^4f377-7]

## Push-Based Updates

Push-based updates use a pure push model: relay WebSocket events flow through three mpsc channels (actor update_tx → on_update callback → nmp_rx → ui_rx) with blocking recv() and zero polling. (Previously: Push-based updates use nmp_app_set_update_callback() registered before nmp_app_start(), sending to a bounded mpsc channel received by the ratatui event loop at the emit_hz rate (4 Hz = 250ms), with no polling.)

<!-- citations: [^4f377-8] [^64f3e-1] -->
## Frame Budget and Performance

The frame budget targets 16.67ms at 60Hz, with ratatui draw kept under 4ms and the iTerm2 ceiling at 30-60 FPS. Blocking I/O in draw(), full-frame image redraws, and unicode_width on CJK characters are avoided to prevent jank. [^4f377-9]

## Animation

Animation uses braille sparklines with 2×4 dot cells, decoupling the data tick (1-2s) from the render tick (30 FPS). [^4f377-10]

## Async Event Loop

The async event loop uses the tokio::select! template over tick/render/crossterm-event/app-action channels. [^4f377-11]

## Profile View

The profile view opens in the left pane with a large 8×4 avatar, name/npub, ✓ Following or [+ Follow] indicator, wrapped bio, stats (following/followers/notes/zapped), and a selectable list of that author's posts; threads from those posts open in the right detail pane. [^93c59-9]

## Action Wiring

sign_in_nsec, wallet_connect, wallet_pay_invoice, and create_account are wired to their actual AppRuntime methods rather than stubbed as toasts. [^93c59-10]

## Session Persistence

Chirp TUI persists the user session across restarts by restoring the keypair, relay list, and UI state. After `nmp_app_start`, `nmp_app_chirp_identity_restore` is called to auto-restore the last identity on boot. The event store uses `LmdbEventStore` instead of the in-memory `MemEventStore` so that kind:10002 relay events survive restarts. Minor UI state (scroll position, selected pane) is persisted in a small `~/.chirp/ui-state.toml` file that chirp-tui owns entirely. [^f8543-1]

Chirp TUI stores account keys in ~/.chirp/config.json instead of the OS Keychain. The ~/.chirp/config.json file is created automatically on first sign-in, written atomically via temp-file + rename, and set to mode 0600 on Unix. The keyring crate and its OS Keychain dependency are not used by chirp-tui. [^16ca6-4]
## See Also

