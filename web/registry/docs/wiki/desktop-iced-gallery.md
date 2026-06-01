---
title: Desktop (Iced) Gallery
slug: desktop-iced-gallery
summary: Desktop (iced) is a registered platform on the nmpui website with the `desktop` value in the `Platform` type, `PLATFORM_ORDER`, and `PLATFORM_LABELS`.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Desktop (Iced) Gallery

## Platform Registration

Desktop (iced) is a registered platform on the nmpui website with the `desktop` value in the `Platform` type, `PLATFORM_ORDER`, and `PLATFORM_LABELS`. [^6a951-8]


## Gallery Components

The desktop gallery implements exactly 6 components: user-avatar, user-name, user-nip05, user-npub, user-card, and embed-article (content-kind-30023). Desktop components not yet implemented by the iced gallery show a 'Desktop soon' disabled tab, consistent with the existing 'Web soon' pattern. [^6a951-9]

## Component Details

Desktop registry entries use the `desktop/content-kind-30023` installId for the article component (defined in `content.ts`, reused by `embeds.ts`), consistent with the swiftui/compose/tui sibling pattern. Desktop embed-article renders title, byline, and summary but has no hero image (the iced `ArticleCard` does not load images yet); this limitation is stated plainly in the registry entry. Desktop `user_nip05.rs` and `user_card.rs` elide the `_@` prefix from NIP-05 identifiers, displaying `f7z.io` instead of `_@f7z.io`. [^6a951-10]

## Known Issues & Fixes

The iced desktop gallery had a subscription bug where the snapshot stream was torn down after 7 frames; it was found and fixed during capture. [^6a951-11]
## See Also

