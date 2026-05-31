---
title: Chirp Desktop Shell Architecture & Views
slug: chirp-desktop
summary: The `crates/nmp-desktop` crate does not exist; it is replaced by `apps/chirp/chirpdesktop/`.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:d1ce3b4a-2a79-40f5-ba0e-fe608f5c7884
  - session:d366b3c7-f7a7-49d5-9961-625037c7deb6
  - session:f5503f3a-d44c-4626-b8de-0492ad1f2a6c
  - session:95a6801d-65a6-481c-985b-4bbe2dbe32c4
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
---

# Chirp Desktop Shell Architecture & Views

## Project Structure

The Chirp desktop application is the native egui/eframe app located at apps/chirp/chirp-desktop, not the iOS target. The crates/nmp-desktop directory is a dead husk that must be removed along with its stale CI exclude, and ADR-0032 doctrine references must be repointed to chirp-desktop. The release manifest (release/nmp-release.toml) must include an entry for apps/chirp/chirp-desktop when it is added to the workspace.

<!-- citations: [^f5503-1] [^d1ce3-1] [^d366b-1] [^95a68-1] [^752b5-3] -->
## FFI Integration

chirp-desktop boots through the same FFI seam as iOS and the TUI. It boots via the `nmp-app-chirp` FFI sequence: `nmp_app_new` → `nmp_signer_broker_init` → `nmp_app_chirp_register` → `nmp_app_start`. It receives FlatBuffer update frames through an FFI callback and decodes them into JSON snapshots. The nmp-desktop kernel bridge streams snapshots via iced::stream::channel.

<!-- citations: [^d1ce3-2] [^d366b-2] -->
## Projections

chirp-desktop has access to all Chirp projections including `nmp.feed.home`, `thread_view`, `author_view`, and `relay_edit_rows`. [^d1ce3-3]

## Actions

chirp-desktop dispatches actions (publish, react, follow, unfollow) through `nmp_app_dispatch_action`. [^d1ce3-4]

## Navigation

The left sidebar contains Home, Profile, and Settings. [^d1ce3-5]

## Home View

The Home view displays a live timeline with note cards, a like button, clickable authors that open their profile, and clickable notes that open their thread. The Compose bar consists of a multiline input and a Publish button, and is only visible on the Home tab. [^d1ce3-6]

## Thread View

The Thread view displays the root note and reply chain with Back navigation. [^d1ce3-7]

## Author View

The Author view displays a profile card, a follow/unfollow button, a note list, and Back navigation. [^d1ce3-8]

## Settings View

The Settings view allows creating an account, signing in with an nsec, viewing a relay list table, and adding a new relay. [^d1ce3-9]

## Status Bar

The Status bar displays Chirp branding, relay health dots, and metrics. [^d1ce3-10]

## See Also

