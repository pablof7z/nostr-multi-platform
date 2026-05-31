---
title: Chirp Desktop App — iced Architecture
slug: chirp-desktop-iced-architecture
summary: The desktop UI framework is iced (version 0.14), not egui
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:d366b3c7-f7a7-49d5-9961-625037c7deb6
  - session:4c4a8c7b-5458-41c1-9ab4-604e7df65a39
---

# Chirp Desktop App — iced Architecture

## UI Framework

The desktop UI framework is iced (version 0.14), not egui. The iced_aw dependency has been removed entirely from the project. [^d366b-1]



The desktop gallery app uses iced instead of egui. [^4c4a8-1]
## Architecture

nmp-desktop uses iced 0.14 Elm architecture with Message, update, view, and subscription. The kernel bridge streams snapshots via iced::stream::channel. [^d366b-2]


When resolving merge conflicts between a master restructure and a PR that replaces egui with iced, the PR's iced version is kept. The `gallery.rs` module must be retained because the iced `main.rs` references `mod gallery`. `lib.rs` remains deleted in the iced version because it is not referenced by `main.rs`. [^4c4a8-2]
## Compose Box

The compose box uses iced's text_editor::Content for action-based, multiline input. [^d366b-3]
## See Also

