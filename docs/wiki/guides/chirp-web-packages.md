---
title: Chirp Web Packages and Build Pipeline
slug: chirp-web-packages
topic: app-web
summary: Chirp web's npm package dependencies are `@nmpis/runtime-web` and `@nmpis/components-web` at `^1.0.0-rc.1`, replacing the old `@nmp/*` scope names
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

# Chirp Web Packages and Build Pipeline

## Dependencies

Chirp web's npm package dependencies are `@nmpis/runtime-web` and `@nmpis/components-web` at `^1.0.0-rc.1`, replacing the old `@nmp/*` scope names. These two packages are independent sibling packages with no cross-dependency; each publishes fully independently, and each generates and vendors its own FlatBuffers wire bindings. Deep imports such as `@nmpis/components-web/src/user-avatar/{ProfileWire,NostrProfileHost,NostrAvatar}` use the package's public `./user-avatar` subpath export.

The `@nmpis` npm packages are published at version `1.0.0-rc.1` with the `rc` dist-tag and `--access public`. The `--tag rc` flag prevents the prerelease from claiming the `latest` dist-tag on first publish.

Publishing `@nmpis/runtime-web` requires setting `CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang` and `AR_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/llvm-ar` on the publish command itself, because Apple's Xcode-bundled clang lacks a WebAssembly backend. The runtime-web npm prepack script triggers a full wasm rebuild on every pack/publish invocation, so the CC/AR env vars must be set on the publish command itself.

<!-- citations: [^dcc80-2aaba] [^dcc80-cd6fc] [^dcc80-1c21a] [^dcc80-11681] [^dcc80-4c57e] -->
## Vite Build & Wasm Staging

Chirp web's production `vite build` uses a `closeBundle` plugin in `vite.config.ts` to stage `@nmpis/runtime-web`'s `dist/wasm/` tree into `dist/assets/` post-build, because Vite never follows the wasm module's internal `new URL(...)` references. This ensures the wasm binary and SQLite vendor snippets land in the production bundle.

<!-- citations: [^dcc80-fa7eb] [^dcc80-78e99] [^dcc80-25a0a] [^dcc80-e64c6] -->
## Vercel & Build Scripts

Chirp web's `vercel.json` and build scripts are repointed at the working `npm install` + `npm run build` path, replacing the stale pre-split monorepo-source-build contract that was dead on arrival.

<!-- citations: [^dcc80-723fd] [^dcc80-ee521] [^dcc80-418c6] -->
## End-to-End Validation

Chirp web is validated end-to-end with Playwright in a real browser: first-run onboarding renders, nsec sign-in connects relays, composer accepts input, and publish fails closed with a Rust-owned diagnostic when no relay target is configured. <!-- [^dcc80-2e5be] -->
