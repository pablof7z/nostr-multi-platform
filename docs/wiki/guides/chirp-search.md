---
title: "Chirp Search: NIP-50 Full-Text and Entity Navigation"
slug: chirp-search
topic: app-codegen
summary: Chirp iOS includes a search feature filed as chirp#71
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

# Chirp Search: NIP-50 Full-Text and Entity Navigation

## Overview

Chirp iOS includes a search feature filed as chirp#71. A magnifier button in the top-right toolbar opens a dedicated full-screen SearchView that is pushed onto the Home tab's NavigationStack. Search supports full-text NIP-50 queries as well as entity navigation: free-text queries run live NIP-50 search with results in reverse-chronological order, cache seeding instantly and relays tailing in; a recognized npub navigates to the profile, a nevent navigates to the thread, and a NIP-AD web URL surfaces as a pinned 'Open …' cell at the top that the user taps to navigate. The NMP search primitives — open_search/NIP-50, ref resolution, and nmp-nip-ad — all exist and are wireable via UniFFI methods in Chirp's app-owned nmp-app-chirp facade, requiring no NMP framework change.

<!-- citations: [^dcc80-3d02a] [^dcc80-88cab] -->
## Input Disambiguation

Disambiguation of search input — distinguishing free text from npub, nevent, naddr, and NIP-AD web URLs — is performed by the Rust intent classifier through a classifyInput facade method. Swift regex is never used for this purpose.

<!-- citations: [^dcc80-b1ff9] [^dcc80-47154] -->
## Wiring

Search is wired through UniFFI methods (openSearch, classifyInput, resolveRef, resolveAd) exposed by the app-owned nmp-app-chirp facade, not via C-ABI. The old C-ABI symbols never existed, which is why chirp#27 previously removed search. <!-- [^dcc80-eb993] -->
