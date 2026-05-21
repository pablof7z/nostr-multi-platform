# Chirp Web Proof

This package is the browser proof for Chirp. It is a Solid/Vite shell that
renders the NMP browser-worker contract and currently reports the explicit
degraded runtime state described in
[`docs/design/chirp-web-runtime.md`](../../docs/design/chirp-web-runtime.md).

## Requirements

- Node.js 20 or newer.
- npm, using the checked-in `package-lock.json`.

## Local Build

From this directory:

```sh
npm ci
npm run build
```

`npm run build` runs TypeScript with `--noEmit` and then writes the static Vite
bundle to `dist/`.

## Local Preview

Build first, then serve the production bundle:

```sh
npm run build
npm run preview -- --host 127.0.0.1 --port 4173
```

Open `http://127.0.0.1:4173/`.

For active development, use:

```sh
npm run dev
```

## Static Deploy

Deploy from `web/chirp`, not the repository root.

Use these settings for static hosts:

| Setting | Value |
| --- | --- |
| Install command | `npm ci` |
| Build command | `npm run build` |
| Output directory | `dist` |
| Node version | `20` or newer |

The current proof has no required environment variables. If the host needs an
SPA fallback, route all paths to `index.html`.

For Vercel, set the project root directory to `web/chirp`, keep the build
command as `npm run build`, and set the output directory to `dist`.
