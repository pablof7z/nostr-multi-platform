---
title: TUI Input Model
slug: tui-input-model
topic: tui
summary: The command palette (`/`) must open context-aware actions based on which pane has focus and whether a reply is selected
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-26
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:95156e27-58fe-4e26-9530-1778033c4559
  - session:1ca92577-a656-4fd9-879e-0f2fd87f0ee7
---

# TUI Input Model

## Command Palette

The command palette (`/`) must open context-aware actions based on which pane has focus and whether a reply is selected. (Previously: the colon-palette `:` for global type-ahead navigation.) The `:` key must be inert and show a one-shot toast reading 'Commands removed — press ? for help or / for palette'. The Help overlay (`?`) must display a scrollable two-column keymap grouped by category (Navigation, Actions on selected note, Feed, Global) for the current tab context, not the palette.

<!-- citations: [^4f377-15] [^4f377-16] [^4f377-17] [^93c59-9] -->
## Input Model

The input model uses four patterns: Pattern A (bottom bar for single-line inputs like nsec/NWC/bolt11/relay URL/zap amount), Pattern B (inline editor for in-place Settings field edits), Pattern C (compose modal for multi-line notes/replies), and Pattern D (modal form for multi-field forms like account creation). Composing a new tweet or reply opens a centered ratatui modal overlay rendered on top of the existing UI chrome. The compose modal displays a wrap-on text area with a block cursor (U+2588) and a hint row showing 'Enter send  Shift+Enter newline  Esc cancel' plus a right-aligned character count. The compose modal title is '✏ New Note' for a new tweet and '↩ Reply to <short_id>' for a reply. Enter sends the message and Shift+Enter inserts a newline. (Previously: Ctrl+Enter to send.) Compose mode requires double-Esc to discard non-empty text (single Esc shows 'Esc again to discard'), preventing accidental data loss. Sensitive input (nsec, bunker URI, NWC URI) in the InputBar must be masked with bullet characters, with Ctrl+R to reveal once. Every mode dispatches key handling off the Mode enum first, before any tab-specific or pane-specific logic. `close_palette()` must only reset mode to Normal when the current mode is still Palette, preventing it from clobbering InputBar mode. The n key is tab-aware: Home → compose new note, Chats → InputBar for DM recipient, Wallet → InputBar for NWC URI, Groups/Settings → toast. The a key opens an account switcher overlay showing all accounts with the active one starred, allowing j/k selection and Enter to switch. AppRuntime methods sign_in_nsec, wallet_connect, and wallet_pay_invoice do not exist and their InputBar handlers currently push 'not yet wired on AppRuntime' toasts; add_relay exists and is fully wired. The input model uses simple multiline composition rather than the tui-textarea crate, to avoid the ratatui 0.29 pin constraint.

Destructive operations (account removal, relay removal, wallet disconnect) require confirmation.

Footer hints are context-sensitive per tab and mode, showing the 6-8 most relevant keys for the current focus.

<!-- citations: [^93c59-11] [^93c59-12] [^4f377-18] [^4f377-19] [^93c59-10] [^95156-2] [^1ca92-2] -->
## Mouse Handling

Mouse capture must use xterm 1002 mode with a documented trade-off that native terminal text selection breaks; Shift-modifier passthrough and an explicit mouse-off toggle must mitigate this. <!-- [^4f377-20] -->
