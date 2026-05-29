---
title: Gallery App Implementations Do Not Satisfy Production Backlog Items
slug: gallery-vs-production-app-distinction
summary: Features implemented only in apps/nmp-gallery/ are not complete — production backlog items require implementation in apps/chirp/.
tags:
  - backlog
  - gallery
  - production
  - verification
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Gallery App Implementations Do Not Satisfy Production Backlog Items

> The repository contains two distinct app targets: the production Chirp app (`apps/chirp/`) and the NMP gallery showcase (`apps/nmp-gallery/`). Features implemented only in the gallery do not satisfy production backlog items, even if the implementation is complete and correct within the gallery context.

## Details

- **Concrete example**: F-CR-05 (iOS NostrKindRegistry) and F-CR-07 (Android NostrKindRegistry) were marked DONE in the backlog, but audit found implementations only in `apps/nmp-gallery/`, not in `apps/chirp/`.
- **Verification rule**: when checking whether a backlog feature is complete, always search both `apps/chirp/` and `apps/nmp-gallery/` separately. A hit only in the gallery means the item is incomplete.
- **Gallery purpose**: `apps/nmp-gallery/` is a component showcase / integration test harness. It is not shipped to end users and does not constitute production delivery.
- **Marking items DONE**: only mark a backlog item DONE when the feature is wired into the production app path (`apps/chirp/`) and reachable by real users.
- **Search pattern**: `grep -r "NostrKindRegistry" apps/chirp/` vs `grep -r "NostrKindRegistry" apps/nmp-gallery/` — presence only in the latter means incomplete.

## See Also
- [[backlog-citations-must-match-head|backlog citations must match head]] — related guide
- [[half-landed-migration-is-not-done|half landed migration is not done]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[chirp-ios-nmp-gallery-component-adoption|Chirp iOS NMP Gallery Component Adoption — Gap Audit and Implementation Plan]] — related guide

- backlog-citations-must-match-head
- half-landed-migration-is-not-done
