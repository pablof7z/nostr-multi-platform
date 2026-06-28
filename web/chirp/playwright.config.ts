import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright configuration for the Chirp Web real-relay acceptance suite
 * (#2038 item E).
 *
 * The webServer block serves the production build via `vite preview`. The build
 * itself does NOT require the wasm artifact (the worker loads it lazily at
 * runtime via a `@vite-ignore` dynamic import), so `vite build` + `vite preview`
 * work whether or not `public/nmp-browser-runtime/` is populated:
 *
 *   • Tests tagged `@wasm` need the real wasm artifact under public/nmp-browser-runtime/ and
 *     run in the nightly full-e2e CI job (see .github/workflows/
 *     chirp-web-acceptance.yml). Without the artifact the worker stays degraded
 *     and these tests fail honestly.
 *   • Untagged tests (the fallback/degraded boot smoke) need no wasm artifact and
 *     run as the PR-blocking browser coverage.
 *
 * Run locally:
 *   npm run build       # vite build (wasm not required for the build step)
 *   npm run test:e2e               # all specs (needs wasm for @wasm tests)
 *   npm run test:e2e -- --grep-invert @wasm   # wasm-free subset only
 */
export default defineConfig({
  testDir: "./tests",

  // Generous outer timeout: wasm cold-start + WS connect can take a few seconds
  // in headless CI; individual heavy specs raise it further via setTimeout.
  timeout: 60_000,

  expect: {
    // Per-assertion timeout — relay round-trips may take a moment.
    timeout: 30_000,
  },

  // The fixture relay binds a random loopback port per spec, so parallel runs
  // are port-safe; keep a single worker for deterministic, low-noise CI logs.
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",

  use: {
    baseURL: "http://localhost:4173",
    headless: true,
    // The NIP-17 fixture relay uses a local self-signed WSS endpoint because
    // production DM relay lists reject non-wss URLs.
    ignoreHTTPSErrors: true,
    trace: "retain-on-failure",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  webServer: {
    // `vite preview` serves the production build from dist/. CI builds first
    // (and, for @wasm tests, builds the wasm artifact first); locally reuse a
    // running server to skip the cold start on every invocation.
    command: "npm run preview",
    url: "http://localhost:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
