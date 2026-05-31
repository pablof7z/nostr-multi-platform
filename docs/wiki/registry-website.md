---
title: Registry Website & Deployment
slug: registry-website
summary: "The registry website uses Solid.js + Vite with `@solidjs/router` and routes for `/`, `/get-started`, and `/components/:id`."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-28
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:5a40faff-56c9-442d-ad96-59432b6f2fea
  - session:3de5a430-eb71-466a-a3d0-eb58e2b42276
  - session:f2fd46d3-1cbd-4f80-9469-0d8137d75478
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
---

# Registry Website & Deployment

## Tech Stack and Routing

The registry website uses Solid.js + Vite with `@solidjs/router`. The 'compose-' prefix is eliminated from all component slugs and routes; components live at unified routes like `/components/user-avatar`. [^45258-30]

The website's `registry.ts` imports Swift and Kotlin source files directly via Vite `?raw` querystring imports, inlined at build time, requiring `server.fs.allow: ["../.."]` in `vite.config.ts`. [^45258-31]

The registry data model organizes components by section (Content, User) with per-component entries containing a platforms map, not by separate platform groups. [^45258-149-154] [^53838-10]

ProfileWire is bundled with NostrAvatar under the 'user-avatar' component; 'user-core' does not exist as a registry entry. [^45258-183-207] [^53838-11]

The Web Registry is a static TypeScript manifest for the public registry docs site that mirrors the CLI `registry.toml` to prevent drift. [^68-71]

When `web/registry` TypeScript components are added, corresponding entries must also be added to `registry.toml` and `cargo run -p nmp-cli --bin nmp -- export jsrepo` must be re-run to regenerate the JSON export.

<!-- citations: [^45258-30] [^45258-31] [^45258-149-154] [^53838-10] [^45258-183-207] [^53838-11] [^53838-9] [^f2605-12] [^54ae9-24] -->
## Deployment

The registry website deploys to nmpui.f7z.io using Vercel with prebuilt local deployment. Deploying via `vercel --prod` from the `web/registry/` subdirectory fails because Vercel only receives that subdirectory and lacks the external source files required by the build. The website's `registry.ts` resolves `?raw` imports of Swift/Kotlin source files from `../../../../crates/nmp-cli/registry/`, which are outside the `web/registry` subdirectory. The working deployment approach is to run `vercel build --prod && vercel deploy --prebuilt --prod` locally from within `web/registry` (where the full filesystem is accessible). All future deployments must use this local prebuilt pattern rather than deploying from the subdirectory. Vercel serves the SPA with a catch-all rewrite rule that routes all paths except `assets/` to `index.html`, allowing SolidJS client-side routing to handle deep URLs. The root `.vercel/project.json` points to the nmp-registry Vercel project, the root `vercel.json` points the build at `web/registry`, and the root `.vercelignore` is configured for the registry project. The `web/registry` project uses `npm install` (not `npm ci`) because it lacks a `package-lock.json`.

<!-- citations: [^45258-1170-1171] [^45258-1299-1300] [^45258-32] [^45258-1602-1606] [^53838-14] [^5a40f-1] [^3de5a-1] [^f2fd4-2] -->
## Platform Switcher

Each component page has a platform switcher bar (Swift / Kotlin / TUI / Web) that swaps the install command, screenshot, code tabs, dependencies, and customization text when toggled. The platform label for Jetpack Compose components is 'Kotlin', not 'Compose'. TUI and Web platform tabs show a 'soon' badge and are disabled. [^45258-25-25] [^45258-149-154] [^45258-173-174] [^45258-50-50] [^53838-12]

## Data Rendering

Registry component pages never show loading spinners; they render best-effort data immediately (e.g. identicon + truncated npub for profiles) and update reactively when kind:0 data arrives from the kernel. [^45258-1239-1249]

NIP-05 identifiers with '_@' prefix (e.g. '_@f7z.io') render without the '_@' prefix (e.g. 'f7z.io') in both Swift and Kotlin badge components. [^45258-531-546] [^53838-13]
## See Also

