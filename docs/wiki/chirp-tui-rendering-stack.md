---
title: Chirp TUI — Rendering Stack and Architecture
slug: chirp-tui-rendering-stack
summary: The Chirp TUI uses ratatui 0.30 with crossterm as its rendering stack
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
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:1572547f-2b2d-49fb-a383-e95ca25d0bc3
---

# Chirp TUI — Rendering Stack and Architecture

## Rendering Stack

The Chirp TUI uses ratatui 0.30 with crossterm as its rendering stack. The layout is a master-detail split: 38% post list on the left, 62% detail pane on the right, with a tab bar at top. ratatui-image 10.0.5 with Picker::from_query_stdio() implements the full image fallback ladder automatically: Kitty → iTerm2 → Sixel → Unicode halfblocks. Inline author avatars use StatefulProtocol/StatefulImage for iTerm2/Sixel/Kitty auto-detection, falling back to colored blocks in unsupported terminals. Rich truecolor rendering is used throughout. The TUI uses a NostrKindRegistry struct with a HashMap of Arc<dyn KindRenderer> handlers propagated via a .kind_registry setter. (Previously: The input model was hybrid with Vim-style j/k, i to compose, and : for the command palette.)

The command palette opens with / and is context-aware: showing post-specific actions (view profile, repost, react, follow, copy ID, zap) when focused on a post, and reply-specific actions when focused on a reply. Pressing : shows a one-shot toast: 'Commands removed — press ? for help or / for palette'.

Four input patterns drive all actions: Pattern A (bottom bar for single-line inputs like nsec, NWC URI, bolt11, relay URL), Pattern B (inline editor in Settings), Pattern C (compose pane taking over right pane for notes/replies, inline strip for DMs), Pattern D (modal form with Tab navigation for multi-field inputs like create account, bunker connect). The TUI uses simple multiline input where applicable.

A client-side profile resolver queries the kernel's profile cache separately, since the snapshot only exposes pubkeys without display_name, picture_url, or nip05.

Animations use Braille sparklines with 2×4 dot cells.

<!-- citations: [^4f377-1] [^4f377-2] [^4f377-3] [^4f377-4] [^4f377-5] [^93c59-2] [^15725-5] -->
## Performance & Frame Budget

The frame budget is 16.67 ms at 60 Hz with ratatui draw kept under 4 ms. Animation data ticks (1–2 s) are decoupled from render ticks (30 FPS). Jank is avoided by keeping blocking I/O out of draw(), avoiding full-frame image redraws, and handling unicode_width carefully on CJK text. [^4f377-6]

Push updates come via nmp_app_set_update_callback() at the emit_hz rate (4 Hz = 250 ms), sending to a bounded mpsc channel consumed by the ratatui event loop with no polling. [^4f377-7]

## Event Loop

The TUI event loop uses tokio::select! over tick, render, crossterm-event, and app-action channels. The update path is a push model with no polling: relay WebSocket → actor ingest → update_tx mpsc → on_update callback → nmp_rx mpsc → ui_rx blocking recv → terminal.draw().

<!-- citations: [^4f377-8] [^64f3e-2] -->
## Layout & Navigation

The relay→feed→event detail hierarchy uses Miller columns as in ranger. Pane navigation uses number-key jumps (1–5) and +/−/_ zoom cycling as in lazygit. Detail pane focus mode is toggled with l/→ to enter and h/←/Esc to return, with j/k navigating through the main post and replies. The active pane gets a cyan border accent. An infobar shows pending-key hints requiring no configuration, as in helix. Contextual help is shown via a ? overlay that displays only bindings valid for the current pane. Mouse clicks work for tab switching, row selection, scroll wheel on both panes, and double-click opens the command palette.

<!-- citations: [^4f377-9] [^93c59-3] -->
## Feed & Thread Model

The multi-pane timeline model uses tut's design for Nostr's parallel feeds (home, mentions, DMs, NIP-29). Nostr threads are rendered as depth-indented flat views rather than tree panes because Nostr threads are DAGs per NIP-10. Unread prioritization follows the weechat hotlist model: mentions/zaps > DMs > reactions > noise. Thread/reply trees, DMs, relay health, and fake nostr data are included in the TUI data model.

<!-- citations: [^4f377-10] [^93c59-4] -->
## Testing & Demo

The CI testing stack comprises TestBackend + insta snapshots + expectrl for PTY-driven end-to-end tests. VHS is used only for non-image flow testing because it cannot render iTerm2/Kitty protocol images in its headless ttyd environment. Image-heavy demos use QuickTime + iTerm2 rather than VHS. [^4f377-11]

## Relay Panel

The relay panel shows all connected relays with health status indicators and a live event count per relay displayed right-aligned in dim text. [^93c59-5]

## Profile View

The profile view opens in the left pane showing the user's avatar, bio, stats, and their posts, with posts/threads opening in the detail pane on the right. The profile pane shows an 8×4 avatar block, name/npub beside it, a Following indicator (green if following, dim Follow button if not), wrapped bio, stats row (following · followers · notes · ⚡ zapped N times), then 'Recent notes ↓' before the filtered post list. [^93c59-6]

## Welcome Screen & Onboarding

The welcome screen renders inline in layout.rs (no standalone onboarding module) when features.accounts is empty, showing 'the nostr social client' text and key hints. n on the welcome screen opens an InputBar labeled 'nsec (or bunker:// URI)' that calls runtime.sign_in_nsec() on Enter. c on the welcome screen opens a ModalForm with a 'Display name' field that calls runtime.create_account() to generate a fresh keypair. An onboarding state machine shows on first launch: welcome → create/import/bunker/browse → relay picker → done, replacing the empty feed with a full-screen flow. [^93c59-7]

## Input Modes & Key Bindings

The footer shows context-sensitive hints in Normal mode: 'n new r reply + react z zap f follow / palette a accounts ? help q quit'. q quits even from Compose mode, and ? toggles help even from Compose mode (previously both were swallowed as text input). n is tab-aware: Home → start compose, Chats → InputBar for DM npub, Wallet → InputBar for NWC connect, Groups/Settings → toast. i toggles inline compose in Chats and Groups tabs. p on the Wallet tab starts an InputBar for bolt11 invoice entry that calls runtime.wallet_pay_invoice() on Enter. a opens the account switcher which calls runtime.switch_account() on Enter to switch the active account. j/k in non-Detail mode dispatches by tab: chat_select_next/prev on Chats, group_select_next/prev on Groups, settings nav on Settings. InputBar, ModalForm, and AccountSwitcher modes render inline in the compose bar area of layout.rs rather than as separate module overlays. [^93c59-8]

## Notifications & Discovery

Three notification surfaces exist: status bar (sync, every keystroke), toast queue (async events above status bar, fade after 5s), and tab badges (persistent unread •N, connection ●, attention ⚠). Tab badges render on the tab bar in the format: [1 Home •3] [2 Chats •2] [3 Groups] [4 Wallet ●] [5 Settings ⚠]. Three discovery tiers exist: context-sensitive footer hints (always visible), ? overlay (full keymap for current context), and Settings→Keys (rebindable full reference). [^93c59-9]

## Title Bar

The title bar shows the active account name, tab labels, relay health dot, and relay count. [^93c59-10]

## Module Organization & Runtime Wiring

All new UI modules (chats, groups, wallet, settings, help) are exported from ui/mod.rs after the user's consolidation commit deleted standalone modules (onboarding, toast, input_bar, modal_form, account_switcher). add_relay is fully wired to AppRuntime; sign_in_nsec, wallet_connect, and wallet_pay_invoice are also wired to AppRuntime methods that exist. [^93c59-11]

## Number Formatting

fmt_sats uses (len - i) % 3 == 0 to insert comma separators, avoiding the subtraction overflow that occurred with (i - first_group). [^93c59-12]
## See Also

