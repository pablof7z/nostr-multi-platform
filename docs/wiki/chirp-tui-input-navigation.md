---
title: Chirp TUI Input, Navigation & Modal System
slug: chirp-tui-input-navigation
summary: The input model is a hybrid of Vim and modal patterns
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-25
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
---

# Chirp TUI Input, Navigation & Modal System

## Input Model

The input model is a hybrid of Vim and modal patterns. In navigation mode, j/k moves the cursor, i toggles inline compose mode in Chats and Groups tabs, and / opens the command palette for searching npubs, hashtags, and kinds. The : command mode is removed; pressing : shows a one-shot toast: 'Commands removed — press ? for help or / for palette'. (Previously: : opened a global type-ahead command palette.)

The command palette opens with /, shows context-sensitive actions (View profile, React, Repost, Follow, Copy note ID, Zap), and dispatches with Enter; reactions update live and other actions show a 2-second toast in the footer. [^93c59-12]

The n key performs a tab-aware action: Home → compose new note, Chats → start DM via InputBar, Wallet → NWC connect via InputBar; on the welcome screen, n opens an InputBar for nsec import and c opens a modal to create a new account. The p key on the Wallet tab opens an InputBar for bolt11 invoice input; on other tabs it opens the author profile. The q key quits the app even from Compose mode; the ? key toggles help even from Compose mode. [^93c59-13]

Four input patterns replace all :command flows: Pattern A (bottom bar for single-line inputs like nsec, NWC URI, bolt11, relay URL), Pattern B (inline editor for in-place field editing in Settings), Pattern C (compose pane for new note/reply/DM messages), Pattern D (modal form with Tab navigation for multi-field inputs like create account and bunker connect). [^93c59-14]

<!-- citations: [^4f377-14] [^93c59-11] -->
## Pane Navigation and Resizing

Multi-pane focus uses lazygit-style 1–5 pane jump keys. Pane resizing uses a +/_ zoom cycle. The detail pane can receive focus via l/→, returning to the list with h/←/Esc; the active pane gets a cyan border accent and the inactive pane dims. When the detail pane has focus, j/k moves through the main post and each reply; the selected item gets a highlight + ▶ gutter; / on a reply shows reply-specific actions against the reply author. Mouse clicks work in the TUI: click tabs to switch, click rows to select, scroll wheel on both panes, double-click opens the command palette.

<!-- citations: [^4f377-15] [^93c59-15] -->
## Help and Hints

Three discovery tiers guide the user: context-aware footer hints (always visible), ? overlay (full keymap for current context), and Settings→Keys (rebindable full reference). An infobox displays pending-key hints without requiring configuration, following the helix pattern.

<!-- citations: [^4f377-16] [^93c59-16] -->

## Notifications

Three notification surfaces exist: status bar (synchronous, every keystroke), toast queue (async events that fade after 5 seconds), and tab badges (persistent: •N unread, ● connection, ⚠ attention). [^93c59-17]
## See Also

