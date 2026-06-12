import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright configuration for the Chirp Web boot smoke (PR-W3).
 *
 * The webServer block starts `vite preview` against the already-built
 * `dist/` directory.  In CI the build step runs before this; locally
 * run `npm run build` (which requires a built wasm package under
 * `public/nmp-wasm/`) before running `npm run test:e2e`.
 *
 * A single Chromium project is enough: the smoke proves the real wasm
 * boots in a real browser; cross-browser coverage is not the goal here.
 */
export default defineConfig({
  testDir: "./tests",

  // Generous outer timeout: wasm cold-start + WS connect can take a
  // few seconds in headless CI.
  timeout: 60_000,

  expect: {
    // Per-assertion timeout — relay Connected status may arrive after a
    // round-trip; give it up to 30 s before failing.
    timeout: 30_000,
  },

  // Run tests sequentially; the fixture relay uses a random port so
  // parallelism would be safe, but a single smoke test needs no concurrency.
  fullyParallel: false,
  workers: 1,

  use: {
    baseURL: "http://localhost:4173",
    // Headless by default; override with --headed for local debug.
    headless: true,
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  webServer: {
    // `vite preview` serves the production build from `dist/`.
    // The CI workflow builds first, then runs Playwright, so `dist/`
    // always exists in CI.  Locally: `npm run build` before this step.
    command: "npm run preview",
    url: "http://localhost:4173",
    // In CI always start a fresh server.  Locally reuse a running one
    // to avoid the cold-start on every `npx playwright test` invocation.
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
