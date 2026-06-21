---
title: TUI Identity Display
slug: tui-identity-display
topic: tui
summary: Per-identity colors must be derived by djb2-hashing the npub (not the mutable display_name) to produce a deterministic color from a fixed palette.
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

# TUI Identity Display

## Per-Identity Color Derivation

Per-identity colors must be derived by djb2-hashing the npub (not the mutable display_name) to produce a deterministic color from a fixed palette. <!-- [^4f377-11] -->

Avatar initials and deterministic avatar color must be derived from the pubkey as fallbacks when profile data is missing. <!-- [^4f377-12] -->

## Display Name Rendering and Disambiguation

Display names must be rendered as `display_name (nip05) · npub1…abcd` so two users with the same display_name never collide, mirroring Mastodon's @user@instance disambiguation. <!-- [^4f377-13] -->

## Profile Display Resolution Fallback Chain

Profile display resolution must use the kernel's fallback chain: display_name → displayName → name. <!-- [^4f377-14] -->

## Profile View Layout

The profile view opens in the left pane (not right) with an 8×4 avatar, name, npub, bio, following/follower counts, and a filtered list of that author's posts. <!-- [^93c59-8] -->
