---
title: "Chirp Groups: Marmot Runtime and Encrypted Keyring Storage"
slug: chirp-groups
topic: app-groups
summary: "chirp#48 is the Groups/Marmot dead-backend bug: a complete, well-built UI wired to a Marmot runtime that the migration removed and never reinstalled"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp Groups: Marmot Runtime and Encrypted Keyring Storage

## Runtime Backend

chirp#48 is the Groups/Marmot dead-backend bug: a complete, well-built UI wired to a Marmot runtime that the migration removed and never reinstalled. Groups/Marmot is re-wired onto `nmp_marmot::install()`, restoring the Marmot runtime backend. Snapshots reach the host, and keyring persistence is verified across restart using real encrypted keyring storage.

<!-- citations: [^dcc80-c9dae] [^dcc80-1efa3] [^dcc80-ea2c9] -->
