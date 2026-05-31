---
title: "NMP Website: Deployment, Design & Voice"
slug: nmp-website
summary: The NMP website is deployed publicly at nmp.f7z.io via Vercel.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-29
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:27e05f9e-7508-4314-82dd-3f83f15b5d8f
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# NMP Website: Deployment, Design & Voice

## Deployment

The NMP website is deployed publicly at nmp.f7z.io via Vercel. [^27e05-1]



The NMP website is deployed publicly at nmp.f7z.io via Vercel. The nmp-registry Vercel project requires a manual deploy (`vercel build --prod && vercel deploy --prebuilt --prod` from web/registry) because it is not auto-deploying from master merges. [^6a951-14]
## Design & Voice

The website design is minimalist with aesthetically pleasing, carefully crafted copy, microcopy, and UX/DX. The voice follows a Basecamp/Ben Settle approach of non-neediness, holding opinions directly without labeling them as philosophy or beliefs. [^27e05-2]


The website platforms are SwiftUI (iOS), Compose (Android), TUI, and Desktop (iced) — not Web, which shows as 'soon'. Desktop components that the iced gallery does not implement show an honest 'Desktop soon' label (same pattern as 'Web soon'). [^6a951-15]
## Content Positioning

Website content is positioned at a level higher than high-velocity technical docs, focusing on philosophical underpinnings rather than code. Source material for the website's positioning is gathered from the repository docs and the RAG/SQLite of kind:1 conversations in the nmp project in ~/.tenex/. [^27e05-3]

## Homepage

The homepage contains no code, no install command, no architecture diagram, and no 'philosophy' section heading. It displays the manifesto lede: 'A broken Nostr app should be impossible to build. Correctness failures in Nostr clients are framework defects. Not developer mistakes.' Below the lede, the homepage displays nine standalone statements as the core content. The strongest statements ('Spinners are a bug.' and 'Cache invalidation is not a concept.') stand alone with no expansion. [^27e05-4]

## /method Page

The /method page contains a longer read with ten rules, thirteen things the framework handles, the audience, the rust-nostr/NDK/Applesauce position, an architecture diagram placed at the bottom under 'The runtime, drawn.', and a sign-off of 'Read the source ↗'. [^27e05-5]

## Header Navigation

The header navigation includes 'Method', 'Source ↗', and a theme toggle, having dropped 'Docs' and renamed 'GitHub' to 'Source'. [^27e05-6]
## See Also

