---
title: NMP Website
slug: nmp-website
topic: website
summary: The NMP developer landing page is deployed to nostr-mp.f7z.io via Vercel production
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-26
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:27e05f9e-7508-4314-82dd-3f83f15b5d8f
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:1231660f-79c1-4b38-9651-9111cc20afb0
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:3de5a430-eb71-466a-a3d0-eb58e2b42276
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
---

# NMP Website

## Overview

The NMP developer landing page is deployed to nostr-mp.f7z.io via Vercel production. The web/registry SolidJS site is the landing page and component registry, not a separate project. The NMP registry website uses Solid.js with Vite and @solidjs/router, importing Swift/Kotlin source files via Vite's ?raw querystring suffix. Navigation links use SolidJS <A> components and client-side routing via @solidjs/router, correctly updating the URL and rendering the target page content. The crates/nmp-cli/registry/ source files are vendored into web/registry/src/vendor/ so the site is self-contained for Vercel deployment. The site deploys from the web/registry/ subdirectory using its own vercel.json with relative commands. The dist/ directory must be rebuilt after adding or changing files in public/ or source, because vite preview serves from dist/. Production deploys for nostr-mp.f7z.io must use `vercel build --prod && vercel deploy --prebuilt --prod` from within `web/registry` rather than a plain `vercel --prod`, because ?raw imports reference files outside web/registry (into crates/) that remote builds cannot resolve. The vercel.json must use a catch-all rewrite rule `/((?!assets/).*)` → `/index.html` so that deep URLs are handled by client-side routing instead of returning 404, while preserving static file serving for assets. (Previously: deployed at nmp.f7z.io; deployed at nmpui.f7z.io.)

<!-- citations: [^27e05-1] [^45258-15] [^12316-1] [^53838-11] [^3de5a-1] [^56d21-7] -->
## Design Principles

The website's design is minimalist with carefully crafted microcopy, copy, and UX/DX. The site's goal is not high-velocity technical docs; it is a bit higher-level than that. The website should show, not tell — even when conveying philosophical underpinnings, it does not label them as such. The voice holds an opinion without labeling it — no 'our philosophy' or 'here's what we believe' preamble. The site voice follows a Basecamp/Ben Settle approach of non-neediness — state the opinion directly, no preamble. <!-- [^27e05-2] -->

## Audience

The NMP website's target audience includes LLM-driven and inexperienced developers. <!-- [^27e05-3] -->

## Visual Design

The site's visual palette is copper-on-near-black with a working light/dark toggle persisted via localStorage. The copper accent color is #E07A3C in dark mode and #C2531A in light mode, on a #0E0F11 warm near-black background. The site uses a density gradient: airy hero, denser content below. Focus rings are non-removable, and the site handles prefers-reduced-motion. <!-- [^27e05-4] -->


Screenshots are stored in public/screenshots/ as PNG files named <component-id>-preview.png and are served from /screenshots/ path. Screenshots must be displayed inside a CSS device mockup (iPhone bezel) with a fixed screen aspect ratio of 9:19.5. The screenshot images inside the device mockup use object-fit: cover and object-position: top to fill the phone frame while anchoring content to the top. The device mockup frame uses #141414 background, 44px border-radius, includes a Dynamic Island pill, volume and power buttons via ::before/::after pseudo-elements, and a home indicator bar at the bottom. The screenshots section uses a flex-wrap layout for multiple screenshots per component flowing left-to-right. <!-- [^12316-2] -->
## Tech Stack

The site's tech stack is Astro 5 + Tailwind 3 + Shiki with self-hosted Inter Tight + JetBrains Mono fonts. The site has zero client-side framework (no React, no Vue, etc.). <!-- [^27e05-5] -->

## Homepage

The homepage contains no code, no install command, no architecture diagram, and no 'philosophy' section heading. The homepage opens with the lede: 'A broken Nostr app should be impossible to build. Correctness failures in Nostr clients are framework defects. Not developer mistakes.' The homepage presents nine statements in scroll order: You don't pick relays. Your iOS code is buttons. Your Android code is buttons. Spinners are a bug. Private events fail closed. Cache invalidation is not a concept. Hardcoded relays belong in the app, not the kernel. One source of truth. Four delivery paths. Errors don't cross the FFI boundary. Reads through subscriptions. Writes through actions. The strongest homepage statements ('Spinners are a bug.' / 'Cache invalidation is not a concept.') stand alone with no expansion. Chirp is the 'built with NMP' social proof anchor, shown as a dedicated mid-page section with one real app. <!-- [^27e05-6] -->


The sidebar is hidden on the home route of the web/registry app, with an app--wide CSS class applied for full-width layout. <!-- [^56d21-8] -->

The StartHere section on the landing page links are ordered: Browse the registry → Scaffold an app → Read the doctrine. <!-- [^56d21-10] -->

The HowItWorks landing page section includes a monospace architecture diagram showing the dispatch/reconcile contract between platform shells and the Rust kernel. <!-- [^56d21-12] -->
## /method Page

The /method page carries a longer read: ten rules, thirteen things the framework handles for you, the audience (including the LLM-driven developer stance), the rust-nostr/NDK/Applesauce position, and the architecture diagram tucked at the bottom under 'The runtime, drawn.' The /method page omits the D0–D10 doctrine section in summary form, linking out instead. The /method page sign-off links to 'Read the source ↗'. <!-- [^27e05-7] -->


The doctrine doc (docs/product-spec/doctrine.md) is linked directly from the marketing page and must be comprehensible to developers who don't know Nostr protocol internals. <!-- [^56d21-11] -->
## Navigation and Footer

The header navigation contains only: Method · Source ↗ · [theme toggle]. The footer tagline is 'Built on rust-nostr.' with no 'Protocol-first' suffix. <!-- [^27e05-8] -->


The Topbar brand is 'nmp' (not 'nmp registry') with nav items: Framework, Registry, Get started, GitHub. <!-- [^56d21-9] -->

The README links to the builder guide at docs/builder-guide/00-how-to-read.md with the description 'the framework guide. Start here for building on NMP.' <!-- [^56d21-13] -->
## Writing Style

The site content avoids em-dash drama, first-person-plural outside the changelog, and the words leverage/empower/robust/just works/seamless/agent-native. CTA style is sentence case with no end punctuation and no urgency verbs. Correctness failures in Nostr clients are positioned as framework defects, not developer mistakes. <!-- [^27e05-9] -->

## Exclusions

The site does not include benchmarks, sponsor walls, testimonials, comparison tables, enterprise CTAs, AI pivots, newsletter popups, chatbots, cookie banners, or mega-menus. The site does not lead with code on the homepage. <!-- [^27e05-10] -->

## 404 Page

The 404 page text is 'That page isn't here — maybe it never was, maybe it moved.' with 'Back to nmp.f7z.io →'. <!-- [^27e05-11] -->

## Branding Assets

The OG image is a 1200×630 dark card with copper wordmark, tagline, and three muted tier bars bottom-right. The favicon is a monogram 'n' in copper Inter Tight. <!-- [^27e05-12] -->

## Source Materials

The philosophy brief is durable at _research/philosophy.md (596 lines) and the revised spec at spec-v2.md. <!-- [^27e05-13] -->
