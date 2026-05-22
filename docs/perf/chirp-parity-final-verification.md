# Chirp Cross-Platform Parity Final Verification

Worker E owns verification strategy for the Chirp parity integration. This file
is the acceptance checklist to run after Workers A/B/C/D are integrated into one
branch. It is intentionally verification-only: production fixes belong in the
owning worker branches unless a tiny test hook is needed.

## Acceptance Checks

1. Shared snapshot shape:
   - `nmp-app-chirp` produces the canonical Chirp snapshot.
   - iOS, web, and TUI consume that snapshot shape without inventing platform
     feed/profile/thread state.
   - `crates/nmp-wasm` worker messages remain JSON-compatible with
     `web/chirp/src/nmp/protocol.ts`.

2. Relay defaults:
   - Default app relays and indexer relays are single-sourced from Rust or from
     Rust-owned config data.
   - Web and TUI must not keep their own divergent hard-coded relay lists.
   - Diagnostics must show the same active relay roles on every platform.

3. Web no-local-policy rule:
   - `web/chirp` may render status, controls, and Rust-produced snapshots.
   - It must not ship fake local timeline/profile rows or maintain divergent
     app relay defaults.
   - Missing browser wasm support must remain an explicit degraded state.
   - When wasm is present, publish-note goes through the Rust-owned Chirp
     action contract and emits a Chirp snapshot the shell only renders.

4. TUI no-local-policy rule:
   - `chirp-repl` commands dispatch into `AppRuntime`.
   - It must not use raw REQ bypasses or local policy paths for app behavior.
   - The TUI parity surface must cover home, profile, thread, search, compose,
     reply, react, follow, unfollow, relay diagnostics, accounts, and MLS.

5. Browser smoke:
   - Production build succeeds.
   - Preview serves the built app.
   - Browser opens the preview, starts the worker, sees the wasm facade reach
     `running`, publishes a note, and sees that note appear from the emitted
     Rust snapshot.

6. CI and Vercel readiness:
   - Required GitHub checks are green or have documented external config
     failures.
   - Vercel builds from `web/chirp` using `npm ci`, `npm run build`, and `dist`.
   - The PR stays draft until these checks pass in the integrated branch.

## Commands

Run the automated local sweep from the integrated branch:

```sh
scripts/chirp-parity-verify.sh
```

Run browser smoke against a production preview:

```sh
cd web/chirp
npm run preview -- --host 127.0.0.1 --port 4173
```

In another terminal from the repo root:

```sh
scripts/chirp-web-browser-smoke.sh http://127.0.0.1:4173/
```

Run CI-equivalent checks when the integration branch is ready:

```sh
cargo test --workspace --exclude nmp-android-ffi --exclude nmp-app-chirp --exclude nmp-desktop
cargo test -p nmp-core --features lmdb-backend
cargo check -p nmp-wasm --target wasm32-unknown-unknown
cd web/chirp && npm ci && npm run build && npm run test && npm audit --audit-level=moderate
```

## Current PR Failure Classification

Checked PR #233 (`codex/chirp-tui-render-metadata`) on 2026-05-22:

- Code checks passing: `cargo test`, `cargo check (android-ffi)`, doctrine grep
  gates, FFI header drift, file-size gate, cargo audit, cargo deny.
- Architecture signoff failing from CI configuration, not code:
  `ARCHITECTURE_REVIEW_PROVIDER or --provider is required`.
- Vercel failing from project/build configuration, not Rust/TUI code:
  deployment runs `vite build` without installing/finding `vite`. The expected
  Vercel settings are root `web/chirp`, install `npm ci`, build
  `npm run build`, output `dist`.

## Integrated Fixes

- The local web sample feed was removed; rows come from worker-emitted
  snapshots.
- Chirp relay bootstrap defaults are single-sourced from
  `nmp-chirp-config` and consumed by web, TUI, REPL, wasm, and core.
- The checked-in browser wasm package loads in the production preview and
  emits a Rust-owned Chirp snapshot for publish-note.

## Known Remaining Boundary

The browser wasm package is not the full live relay actor. A direct
`cargo check -p nmp-core --target wasm32-unknown-unknown` fails on this machine
because native crypto dependencies (`ring`, `secp256k1-sys`) cannot be compiled
by the available Apple clang wasm target. Until that toolchain/feature-gating
work lands, the browser proof demonstrates shared Rust action/snapshot/relay
contracts rather than live relay I/O.
