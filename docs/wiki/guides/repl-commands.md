---
title: REPL Commands
slug: repl-commands
topic: developer-workflow
summary: "The create-account REPL command accepts inline relay URLs (e.g., create-account alice wss://relay.primal.net)"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-19
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
---

# REPL Commands

## create-account

The create-account REPL command accepts inline relay URLs (e.g., create-account alice wss://relay.primal.net). The first non-wss:// argument is parsed as the display name, and any remaining wss:// arguments are parsed as relays. <!-- [^fe79b-11] -->
