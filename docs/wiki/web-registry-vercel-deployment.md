---
title: Web Registry Vercel Deployment — Build and Deploy Pattern
slug: web-registry-vercel-deployment
summary: The web registry must be deployed from the repo root using the `vercel build --prod` then `vercel deploy --prebuilt --prod` pattern, not by running `vercel --pr
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-26
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:5a40faff-56c9-442d-ad96-59432b6f2fea
  - session:3de5a430-eb71-466a-a3d0-eb58e2b42276
  - session:f2fd46d3-1cbd-4f80-9469-0d8137d75478
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
---

# Web Registry Vercel Deployment — Build and Deploy Pattern

## Deployment

The web registry must be deployed using `vercel build --prod && vercel deploy --prebuilt --prod` from within the `web/registry` directory, rather than `vercel --prod`, because `?raw` imports and the Vite config reference `.swift` files from `../../../../crates/nmp-cli/registry/swiftui/`, which are outside `web/registry/` and unavailable to Vercel's remote builder. Crates/nmp-cli/registry/ source files are vendored into `web/registry/src/vendor/` so the site is self-contained for Vercel deployment. The Vercel project `rootDirectory` setting must not be set to `web/registry`, as it is a git-integration setting that causes path doubling when used with CLI deploys. The project should use Vercel's git integration so it clones the full repository and handles the monorepo build automatically. The root `vercel.json` points build output to `web/registry`. The root `.vercel/project.json` targets the `nmp-registry` Vercel project. The root `.vercelignore` is configured for the registry project, excluding `web/chirp/`.

<!-- citations: [^5a40f-2] [^5a40f-3] [^3de5a-1] [^f2fd4-1] [^56d21-9] -->
## See Also

