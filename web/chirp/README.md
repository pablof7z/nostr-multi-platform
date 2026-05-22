# Chirp Web Proof

This package is the browser proof for Chirp. It is a Solid/Vite shell that
renders the NMP browser-worker contract. Product actions are sent as typed
Chirp intents; the checked-in `nmp-wasm` package maps those intents and emits
Rust-owned Chirp snapshots for the shell to render.

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

The repository root includes `vercel.json` so a Vercel project pointed at the
repo root still installs and builds `web/chirp`.

Use these settings for static hosts:

| Setting | Value |
| --- | --- |
| Install command | `cd web/chirp && npm ci` |
| Build command | `cd web/chirp && npm run build` |
| Output directory | `web/chirp/dist` |
| Node version | `20` or newer |

If the host needs an SPA fallback, route all paths to `index.html`.

## Wasm Package

The browser worker loads a generated `nmp-wasm` package from:

```text
public/nmp-wasm/nmp_wasm.js
```

Refresh it after changing `crates/nmp-wasm`:

```sh
wasm-pack build ../../crates/nmp-wasm --target web --out-dir ../../web/chirp/public/nmp-wasm
```

When that package is absent, the worker emits
`wasm_bridge_unavailable` and falls back to `DegradedRuntime` with
`browser_bridge_unavailable` status.

The wasm facade is intentionally lightweight: it proves the browser uses the
same Rust-owned action contract, relay defaults, and Chirp snapshot shape. Full
live relay I/O still belongs to the shared actor driver; `nmp-core` does not
yet compile to browser wasm on this toolchain because native crypto C
dependencies fail for `wasm32-unknown-unknown`.
