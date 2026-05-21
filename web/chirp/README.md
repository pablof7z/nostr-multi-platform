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

The repository root includes `vercel.json` so a Vercel project pointed at the
repo root still installs and builds `web/chirp`.

Use these settings for static hosts:

| Setting | Value |
| --- | --- |
| Install command | `cd web/chirp && npm ci` |
| Build command | `cd web/chirp && npm run build` |
| Output directory | `web/chirp/dist` |
| Node version | `20` or newer |

Optional runtime config:

| Variable | Meaning |
| --- | --- |
| `VITE_CHIRP_RELAYS` | Comma-separated relay URLs passed to the Rust worker `start` config. |
| `VITE_CHIRP_DATABASE` | Browser database name; defaults to `chirp-web`. |

If the host needs an SPA fallback, route all paths to `index.html`.

## Optional Wasm Package

The browser worker first tries to load a generated `nmp-wasm` package from:

```text
public/nmp-wasm/nmp_wasm.js
```

That file is optional for normal web builds. When it is absent, the worker emits
`wasm_bridge_unavailable` and falls back to `DegradedRuntime` with
`browser_bridge_unavailable` status.

If the generated module loads, the worker routes requests through
`NmpWasmRuntime.handle_json()`. Any `browser_actor_driver_missing` status then
comes from the real wasm runtime, which means the JS/wasm bridge is available
but the browser actor driver is still not linked.
