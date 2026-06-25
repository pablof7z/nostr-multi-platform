# Chirp Web Proof

This package is the browser proof for Chirp. It is a Solid/Vite shell that
renders the NMP browser-worker contract. Product actions are sent as typed
Chirp intents; the `nmp-wasm` package maps those intents and emits Rust-owned
Chirp snapshots for the shell to render.

## Requirements

- Node.js 20 or newer.
- npm, using the checked-in `package-lock.json`.
- Rust stable + `wasm32-unknown-unknown` target (for the wasm build step).
- `wasm-pack` 0.13.1 (`cargo install wasm-pack --version 0.13.1 --locked`).
- `clang` with wasm32 support (required by secp256k1-sys when
  cross-compiling to wasm32). On macOS, install Homebrew LLVM; the npm and
  deploy scripts prefer `/opt/homebrew/opt/llvm/bin` and
  `/usr/local/opt/llvm/bin` automatically.

## Local Build

The wasm package is generated at build time and **not checked in**
(`public/nmp-wasm/` is gitignored).  Build it first, then build the web app:

```sh
cd web
npm install
npm run build:wasm -w @nmp/chirp-web   # compiles apps/chirp/crates/nmp-app-chirp-web -> chirp/public/nmp-wasm/
npm run build -w @nmp/chirp-web        # codegen + TypeScript check + Vite bundle -> chirp/dist/
```

`build:wasm` requires `wasm-pack` on `$PATH` and `CC_wasm32_unknown_unknown=clang`
(the script sets this automatically).  If Rust is not on your machine,
`scripts/build.sh` installs it automatically (used by the Vercel deploy).

## Local Preview

Build first, then serve the production bundle:

```sh
cd web
npm run build:wasm -w @nmp/chirp-web
npm run build -w @nmp/chirp-web
npm run preview -w @nmp/chirp-web -- --host 127.0.0.1 --port 4173
```

Open `http://127.0.0.1:4173/`.

For active development, run the wasm build once, then use:

```sh
cd web
npm run dev -w @nmp/chirp-web
```

## Static Deploy

`web/chirp/vercel.json` sets `"buildCommand": "bash scripts/build.sh"`,
which installs Rust + wasm-pack if absent, builds nmp-app-chirp-web, and runs
`npm run build`.  The Vercel project must have its root directory set to
`web/chirp` so the script can reach `apps/chirp/crates/nmp-app-chirp-web`.

For other static hosts:

| Setting | Value |
| --- | --- |
| Install command | `cd web && npm install` |
| Build command | `cd web/chirp && bash scripts/build.sh` |
| Output directory | `web/chirp/dist` |
| Node version | `20` or newer |

If the host needs an SPA fallback, route all paths to `index.html`.

## Wasm Package

The browser worker loads a generated `nmp-wasm` package from:

```text
public/nmp-wasm/nmp_app_chirp_web.js
```

The package is produced by `wasm-pack build --target web apps/chirp/crates/nmp-app-chirp-web` and
is gitignored — a fresh build is always required.  `npm run build:wasm`
wraps the invocation with the correct `CC_wasm32_unknown_unknown=clang`
environment variable.

When the package is absent, the worker emits `wasm_bridge_unavailable` and
falls back to `DegradedRuntime` with `browser_bridge_unavailable` status.

The wasm facade proves the browser uses the same Rust-owned action contract,
relay defaults, and Chirp snapshot shape.

## NMP Worker Protocol

`web/chirp` is a consumer of the framework-level WASM worker contract defined
in `crates/nmp-wasm`. The framework contract is documented at:

- **ADR-0047** (`docs/decisions/0047-browser-worker-runtime.md`) — the
  decision: Worker-loop-as-actor, sync read path, async write path, binary
  snapshot frames, honest degraded modes.
- **`docs/wasm-surface.md`** — the living reference: `WorkerRequest` /
  `WorkerEvent` tables, dispatch paths, snapshot callback, degraded-mode
  vocabulary.

This shell treats `RuntimeStatus` as first-class UI state: the worker surfaces
`runtime_status` events rather than synthesising a healthy-looking UI while
capability gaps exist.

### Parity fixtures (follow-on)

Cross-platform snapshot comparison between this web shell and iOS, Android,
desktop, and TUI for the same action history is not yet implemented. When it
lands it will compare `UpdateFrame` bytes produced by the wasm runtime against
those produced by the native actor for the same action sequence.
