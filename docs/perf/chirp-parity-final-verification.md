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
   - It must not ship fake local timeline/profile rows or decide product state
     while the worker is degraded.
   - Missing browser runtime support must remain an explicit degraded state.

4. TUI no-local-policy rule:
   - `chirp-repl` commands dispatch into `AppRuntime`.
   - It must not use raw REQ bypasses or local policy paths for app behavior.
   - The TUI parity surface must cover home, profile, thread, search, compose,
     reply, react, follow, unfollow, relay diagnostics, accounts, and MLS.

5. Browser smoke:
   - Production build succeeds.
   - Preview serves the built app.
   - Browser opens the preview, starts the worker, sees the expected degraded
     status or live runtime status, and publish dispatch reports through the
     worker event log.

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

## Current Integration Blockers

These are blockers for the final integrated branch, observed on this Worker E
base before Worker A/B/C output is merged:

- `web/chirp/src/App.tsx` still contains a local sample `initialFeed`; that
  violates the web no-local-policy acceptance rule unless removed or replaced
  by Rust snapshot rendering.
- `web/chirp/src/nmp/client.ts` and `crates/chirp-repl/src/session.rs` currently
  define different default relay sets. Relay defaults need to be single-sourced
  before final parity signoff.
