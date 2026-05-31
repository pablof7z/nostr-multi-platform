---
title: Chirp Compose View
slug: chirp-compose-view
summary: The compose screen displays a circular progress ring (ComposeProgressRing) instead of a plain character count number
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-26
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:1ca92577-a656-4fd9-879e-0f2fd87f0ee7
---

# Chirp Compose View

## Compose Progress Ring

The compose screen displays a circular progress ring (ComposeProgressRing) instead of a plain character count number. The ring fills as characters are used, turns orange when 20 or fewer characters remain, and turns red when the character limit is exceeded. [^19e07-3]


## Compose Modal Overlay

Composing a new tweet or reply in chirp-tui opens a centered modal overlay with a Clear-backed background and ACCENT_CYAN border. [^1ca92-1]

Pressing Enter in the compose modal publishes the tweet or reply. Pressing Shift+Enter inserts a new line. [^1ca92-2]
## See Also

