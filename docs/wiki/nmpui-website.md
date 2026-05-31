---
title: nmpui.f7z.io — Component Showcase Website
slug: nmpui-website
summary: The nmpui.f7z.io website is a SolidJS/Vite app at web/registry/ that showcases every nmp-gallery component with real screenshots across platforms; screenshots are taken via xcrun simctl on iOS and render tests on TUI.
tags:
  - nmp-gallery
  - website
  - screenshots
  - solidjs
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-26
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:c8c2902c-43a6-4b1c-8215-1732dc266895
  - session:5a40faff-56c9-442d-ad96-59432b6f2fea
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
---

# nmpui.f7z.io — Component Showcase Website

> The nmpui.f7z.io website is a SolidJS/Vite app at web/registry/ that showcases every nmp-gallery component with real screenshots across platforms; screenshots are taken via xcrun simctl on iOS and render tests on TUI.

## Architecture

The website is a SolidJS/Vite application located at web/registry/ in the repository that serves as both the component registry and the developer landing page — they are integrated, not separate. The production deployment targets https://nmpui.f7z.io (aliased as nmp-registry.vercel.app), and the NMP developer landing page deploys to nostr-mp.f7z.io via Vercel production. Screenshots live in public/screenshots/. The site renders a component catalog organized by section (User, Content, Relay, Embeds & Kinds) with screenshots for each component variant across platforms (iOS/swiftui, Android/compose, TUI, Web, and Desktop/iced). Android/Compose screenshots follow the naming convention '<component>-kotlin-preview.png'. Desktop tabs show 'Desktop soon' for components not yet implemented. The Embeds & Kinds section includes an Android (compose) column with verified screenshots for embed-article, embed-profile, embed-note, and embed-highlight. All 5 user component entries in user.ts have their Compose screenshots populated: user-avatar, user-name, user-nip05, user-npub, and user-card. As of the last audit, the Embeds section was completely missing from the web registry even though it exists in apps/nmp-gallery/registry.json. Screenshot files follow the naming convention '{slug}-{platform}-preview.png' (e.g. 'user-avatar-swift-preview.png'). Gallery demo data uses pablof7z's pubkey (fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52) and npub (npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft). The sidebar is hidden on the home route, and the app--wide CSS class provides a full-width layout.

<!-- citations: [^6a951-41] [^6a951-132] [^53838-11] [^c8c29-3] [^5a40f-1] [^56d21-5] -->
## Screenshot Manifest Requirements

A comprehensive Sonnet agent must produce a full manifest of every component to screenshot before any screenshots are taken. For each component, the manifest must specify what to verify: author display (not pubkey), titles, surrounding context text, images (avatars, article embeds), identicon fallbacks, and all variants. The screenshot process doubles as a quality check — every component must render correctly with real data from the kernel, no hacks. [^6a951-42]

## Screenshot Capture Method

iOS screenshots are taken via xcrun simctl save to files directly to web/registry/public/screenshots/. The iOS gallery is built from the Rust library (libnmp_app_chirp.a built for iOS simulator) and launched on a simulator. TUI screenshots are captured via render tests that output component render output. The process navigates through every component systematically across all sections: User, Content, Embeds, and Relay. [^6a951-43]


iOS screenshots are captured via xcrun simctl save to files directly to web/registry/public/screenshots/. During the comprehensive screenshot pass, three iOS code fixes were required to make components render correctly with real data: (1) UserProfileNamePage and UserNip05Page lacked NostrProfileHost context — they passively received ProfileWire but never claimed the profile, causing display names to show as npub. The fix gives them pubkey: input and a NostrAvatar to own the claim, matching UserAvatarPage. (2) ProfileEmbedPage read from snapshot.profiles[pubkey] but nothing on the page claimed the profile — the fix uses the same pattern: a NostrAvatar to own the claim. (3) embed-highlight event (kind:9802) was not on the configured showcase relays — the showcase-references.json relay list was missing nos.lol, which carries the event. These fixes landed in PR #820 together with all refreshed screenshots. [^6a951-81]

Additional iOS embed fixes after PR #820: (1) EmbedHost.swift was fixed to plumb authorDisplayName and authorPictureUrl from ClaimedEventDto through to EmbedKindProjection constructors — the kernel already emitted these fields in the JSON, and the projection types already accepted them, but the DTO decode step was ignoring them. After the fix, article embeds show 'Gigi' (not '6e468422…') and note embeds show 'PABLOF7z' (not 'fa984bd7…'). (2) The showcase highlight event was not on the configured relays — the nevent was re-encoded using nak with wss://nos.lol as the relay hint, after which the highlight resolved showing 'Vibe-coding is what brought me back to programming.' [^6a951-99]

Screenshots must be full-device captures (not cropped portions) with the correct iPhone aspect ratio for display in the DeviceMockup CSS frame. The device mockup CSS uses 'object-fit: contain' with a '#f2f2f7' background so full screenshots display without zooming or cropping. [^53838-12]
## Missing Sections

The web registry now has all four sections matching registry.json: User, Content, Relay, and Embeds & Kinds. The Embeds section was added in PR #819 with its four components (embed-article, embed-profile, embed-note, embed-highlight) and corresponding screenshot arrays. TUI screenshot references for relay-list and all embed components were added in a follow-up TypeScript update. The current live state on master includes 16 iOS gallery + 16 TUI component screenshots, all with relay-resolved data (PABLOF7z showing as display name everywhere, not hex/npub). The Vercel deployment picks up from master automatically.

<!-- citations: [^6a951-44] [^6a951-79] -->
## Screenshot Quality Gates

Each screenshot must verify specific quality attributes without hacks: for article embeds, the screenshot must show the author (not pubkey), the article title, surrounding note text (e.g. 'hey, check out my new article [card] I hope you like it!'), and all images (avatar, article embed hero image) rendered correctly. For user components, display names (not pubkeys) must be visible. For identicon fallback, an empty avatar URL must produce the geometric identicon. [^6a951-45]


The embed inline-flow requirement: every event embed must render inline within its surrounding note text. The pattern is 'hey, check out my article' → [medium-like article card] → 'I hope you enjoy it!' Verification fails if the raw nostr:naddr1… shows as plain text, if the embed card swallows the surrounding prose, or if the surrounding text is missing entirely. This applies to all embed types on all platforms. Additionally, no blank image placeholders are accepted — 'probably ok' is banned; images must actually render (avatar photos, article hero images, media grid thumbnails). The screenshot capture process doubles as a quality check: any component that fails to render correctly is a bug, not a screenshot timing issue to work around. [^6a951-80]

The login-block showcase renders an honest fallback state showing manual key-entry and an install hint for external signers, not a blank or loading state. [^6a951-133]

## Deployment

The registry deploys to nmpui.f7z.io via Vercel using 'vercel build --prod' locally then 'vercel deploy --prebuilt --prod' (because registry.ts imports files outside web/registry/ that aren't available to Vercel's remote build). The vercel.json must NOT contain a catch-all 'rewrites' rule because it blocks static file serving (screenshots, JSON); Vercel's Vite preset handles SPA fallback automatically. [^53838-13]

## StartHere Ordering

StartHere ordering is: Browse the registry (01), Scaffold an app (02), Read the doctrine (03) — doctrine is not first. [^56d21-6]
## See Also
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[nmp-gallery-verification-matrix|NMP Gallery Verification Matrix — 64-Cell Cross-Platform Quality Gate]] — related guide
- [[embed-inline-flow-rendering|Embed Inline-Flow Rendering — Cards Within Surrounding Prose]] — related guide

