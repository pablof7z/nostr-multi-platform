---
title: egui
slug: egui
summary: egui is an immediate-mode GUI library used to build the Chirp desktop application's user interface, providing layout, rendering, and input handling.
tags:
  - egui
  - gui
  - desktop
  - chirp
  - immediate-mode
volatility: warm
confidence: low
created: 2026-05-29
updated: 2026-05-29
verified: 
compiled-from: codebase
sources:
  - codebase
---

egui is an **immediate‑mode GUI library** for Rust, used in this project to implement the Chirp desktop client. It renders the JSON snapshot projections produced by the Chirp kernel (<a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/main.rs#L5'>(main.rs:5)</a>). The desktop shell is bootstrapped via `eframe`, which accepts an `egui::ViewportBuilder` to configure the window size and title (<a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/main.rs#L18'>(main.rs:18)</a>). The application logic lives in `DesktopApp`, which implements the `egui::App` trait to receive `&egui::Context` and drive the UI lifecycle (<a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/app.rs#L1'>(app.rs:1)</a>, <a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/app.rs#L106'>(app.rs:106)</a>). Inside `update()`, the code uses `egui::CentralPanel`, `egui::SidePanel`, `egui::TopBottomPanel`, and various widgets such as `egui::Button`, `egui::ComboBox`, and `egui::Grid` (<a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/app.rs#L10'>(app.rs:10)</a>, <a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/app.rs#L267'>(app.rs:267)</a>, <a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/app.rs#L562'>(app.rs:562)</a>, <a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/app.rs#L595'>(app.rs:595)</a>).

Content rendering in <a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/render.rs'>(render.rs)</a> imports `egui::{Color32, RichText, Ui}` to create styled note bodies with colours for hashtags, mentions, media links, and more (<a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/render.rs#L7'>(render.rs:7)</a>). A helper function `hex_color()` parses `#rrggbb` strings directly into an `egui::Color32` (<a href='/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/wf_5b4df4ca-2ea-7/apps/chirp/chirp-desktop/src/render.rs#L29'>(render.rs:29)</a>). The library provides the immediate‑mode infrastructure: layout management, rendering, and input handling for the entire desktop application.

Additionally, a core data structure in `nmp-core` borrows its design from `egui::Id`. The `SubKey` builder uses the same *hashed typed tuple* pattern to create stable, allocation‑free identifiers for subscriptions (<a href='/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/subs/sub_key.rs#L9'>(sub_key.rs:9)</a>, <a href='/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/subs/sub_key.rs#L33'>(sub_key.rs:33)</a>, <a href='/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/subs/sub_key.rs#L59'>(sub_key.rs:59)</a>). Thus egui influences both the runtime GUI layer and the internal architectural patterns of the project.
