---
title: TUI Event Loop
slug: tui-event-loop
topic: tui
summary: Data tick (1â2s) must be decoupled from render tick (30 FPS) to avoid jank while keeping the UI responsive
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
---

# TUI Event Loop

## Data vs Render Tick Decoupling

Data tick (1–2s) must be decoupled from render tick (30 FPS) to avoid jank while keeping the UI responsive. <!-- [^4f377-4] -->

The frame budget must keep ratatui draw under 4ms and stdout flush under 8ms per frame at 60Hz. <!-- [^4f377-5] -->

The snapshot emit rate is controlled by emit_hz (4 Hz = 250ms in chirp-repl), and snapshots only fire when kernel.changed_since_emit() is true (no idle ticks). <!-- [^4f377-6] -->

## Async Event Loop

The chirp-tui event loop uses a pure push model: actor → update_tx mpsc → update listener thread → C callback on_update → nmp_rx mpsc → ui_rx.recv() blocking → terminal.draw(), with zero polling and zero timers. The kernel push-update mechanism must be used for non-polling TUI updates: register nmp_app_set_update_callback() before nmp_app_start(), with the callback sending to a bounded mpsc channel that wakes the ratatui event loop. Update callbacks and snapshot projectors must be cheap and non-blocking (doctrine D8); a blocking callback on the listener thread stalls all subsequent updates. The status bar must only be mutated synchronously inside key handlers — never from async tasks — to preserve the e2e test oracle invariant.

<!-- citations: [^4f377-7] [^4f377-8] [^4f377-9] [^93c59-6] [^64f3e-7] -->
