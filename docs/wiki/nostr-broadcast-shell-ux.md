---
title: Nostr Broadcast Shell UX
slug: nostr-broadcast-shell-ux
summary: The app displays a prompt where the user can type a note
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:b6578d9e-697f-41ae-ab75-5e5643ceff13
---

# Nostr Broadcast Shell UX

## User Experience

The app displays a prompt where the user can type a note. Hitting Enter dispatches a PostNote action through the NMP kernel. When the note is published, the app displays a confirmation with the note text. Output uses simple println! style rather than a fancy TUI library. [^b6578-8]


## Rust Binary

The Rust binary in examples/shell.rs creates the kernel using the NmpAppBuilder pattern from the builder guide. It handles the signer/session setup using the generate new key flow from the builder guide, and generates or loads a keypair using the action for this. The binary accepts stdin input in a loop, dispatches the PostNote action for each line entered, and prints the result of the dispatched action. [^b6578-9]

## Build Location

Everything is built in /tmp/nostr-broadcast/. [^b6578-10]
## See Also

