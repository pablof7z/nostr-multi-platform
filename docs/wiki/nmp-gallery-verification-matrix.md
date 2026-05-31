---
title: NMP Gallery Verification Matrix — 64-Cell Cross-Platform Quality Gate
slug: nmp-gallery-verification-matrix
summary: Comprehensive 64-cell verification matrix across 4 platforms × 16 components with inline-flow, no-hacks rules, and a PDF deliverable at docs/testing/nmp-gallery-verification-report.pdf.
tags:
  - testing
  - nmp-gallery
  - verification
  - screenshots
volatility: hot
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# NMP Gallery Verification Matrix — 64-Cell Cross-Platform Quality Gate

> Comprehensive 64-cell verification matrix across 4 platforms × 16 components with inline-flow, no-hacks rules, and a PDF deliverable at docs/testing/nmp-gallery-verification-report.pdf.

## Overview

Every nmp-gallery component must be verified as working correctly on every platform before the work can be called DONE. Verification is tracked in a comprehensive 64-cell matrix covering 4 platforms × 16 components, stored at docs/testing/nmp-gallery-verification-matrix.md. After all cells are verified, a PDF report combining the matrix and every screenshot is generated at docs/testing/nmp-gallery-verification-report.pdf so the user can independently judge whether the 'it works' claim is true. [^6a951-63]

## Matrix Scope — 64 Cells

Platforms: iOS · Android · TUI · Desktop. Components: 16 total across 4 sections — 5 User (user-avatar, user-name, user-nip05, user-card, user-about), 1 Relay (relay-list), 6 Content (content-note, content-article, content-quote-card, content-mention-chip, content-media-grid, content-minimal), 4 Embed (embed-article, embed-profile, embed-note, embed-highlight). Each cell is only checked after seeing it render correctly on the running app. [^6a951-64]

## Ground Truth Data

Verification uses real data from showcase-references.json as ground truth. Profile components must show pablof7z as the display name (never hex or npub), with real avatar photo and NIP-05 badge on the correct domain. Article embed must show 'What's left of the internet?' by Gigi with summary and hero image rendered. Note embed must show 'grok cli is INSANELY bad, jesus' by pablof7z. Highlight embed must show 'Vibe-coding is what brought me back to programming' by pablof7z. Relay list must show purplepag.es (Indexer) and relay.primal.net (Both+Indexer) with live status dots. [^6a951-65]

## Embed Inline-Flow Requirement

Every event embed must render inline within its surrounding note text. The pattern is: 'hey, check out my article' → [medium-like article card] → 'I hope you enjoy it!' The verification FAILS if: the raw nostr:naddr1… shows as plain text, the embed card swallows the surrounding prose, or the surrounding text is missing entirely. This applies to all four embed types on all four platforms. [^6a951-66]

## No-Hacks Rules Per Cell

Every cell must pass these rules: (1) No hex where a display name belongs — author bylines must show resolved names. (2) No 'Loading…' or 'Fetching…' as a final state — the component must eventually render real data. (3) No blank image placeholders — 'probably ok' is banned; images must actually render (avatar photos, article hero images, media grid thumbnails). (4) No shell pre-warming — the component, not the shell, must drive data fetching. (5) No hidden claim-trigger components — if a component needs a profile, it must own the claim openly. (6) Kernel never fetches kind:0 off an event — profile resolution is presentation-layer, not ingest-side. [^6a951-67]


Screenshot methodology rule: do not force-stop the app between components. A cold kernel start means kind:0 has not resolved within the typical 12-second screenshot window, producing npub fallback. The correct methodology for the final screenshot pass is one warm session, navigate without restarting, and let profiles resolve before capturing. This is a methodology requirement, not a code bug. [^6a951-95]
## PDF Deliverable

A verification PDF must be generated combining the test matrix and all labeled screenshots so the user can independently falsify or confirm every 'it works' claim. After all 64 cells are verified on the running apps, the PDF report is generated at docs/testing/nmp-gallery-verification-report.pdf. It combines the verification matrix with every labeled screenshot placed beside its pass/fail criteria. Every screenshot must be verified to show resolved display names (not pubkeys/hex), real images (not blank placeholders), article titles, inline surrounding note text, and formatted NIP-05 domains. The PDF always shows the real screenshot with honest per-cell ✓/⚠/✗ annotations rather than replacing failing cells with placeholder images. This allows the user to falsify the claim, not just confirm it — every cell's evidence is directly inspectable. Nothing is called DONE until this PDF exists and honestly reflects every cell. The PDF spec is recorded in the verification matrix doc itself.

<!-- citations: [^6a951-68] [^6a951-115] [^6a951-130] -->
## Blockers

A claimed-event regression (suspected from PR #828, EventStore clock threading) blocks all embed cells on all platforms — event embeds are stuck 'loading embedded event' while only the profile mention resolves. Until the root-cause agent fixes this, every embed-article, embed-note, embed-profile, and embed-highlight cell is FAIL on every platform. [^6a951-69]


Embed-blocking root cause is a data-reachability issue, not a code regression. The two relays the gallery seeds (purplepag.es + relay.primal.net) do not carry the showcase note, article, or highlight events. These events exist on nos.lol, which the gallery does not seed. All suspected code regressions (#825, #828, #834, #836, #841) were exonerated. The fix for the showcase is to re-encode the nevents with nos.lol as the relay hint. After the relay-hint fix, the note event resolves at the kernel level (EventRx + terminal_hit confirmed via NMP_CLAIM_LOG), but a separate kind:1 projection gap was discovered: the note appears in the kernel's event resolution path but does not reliably land in the claimed_events projection. [^6a951-96]

## Screenshot Methodology

Do not force-stop the app between components. A cold kernel start means kind:0 has not resolved within the typical 12-second screenshot window, producing npub fallback. The correct methodology for the final screenshot pass is one warm session, navigate without restarting, and let profiles resolve before capturing. Every registry screenshot entry must point to a verified post-fix capture rather than pre-fix captures showing hex bylines, 'loading' states, wrong pages, or raw epochs. Screenshots must be verified via accessibility tree output rather than assumed from pixels alone. Broken observations on the running app must be investigated rather than explained away as flakiness; a passing compile does not mean a working feature. [^6a951-131]
## See Also
- [[nmpui-website|nmpui.f7z.io — Component Showcase Website]] — related guide
- [[embed-inline-flow-rendering|Embed Inline-Flow Rendering — Cards Within Surrounding Prose]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[showcase-relay-data-reachability|Showcase Relay Reachability — Data Lives on nos.lol, Not Default Seeds]] — related guide

