import { defineConfig, devices } from "@playwright/test";

// Verifies the gallery boots the real NMP wasm kernel in a real browser and
// resolves a real profile from real relays. The webServer runs `vite preview`
// against the built dist/ (which includes public/nmp-browser-runtime). Build first:
//   npm run build:wasm && npm run build
export default defineConfig({
  testDir: "./tests",
  timeout: 90_000,
  expect: { timeout: 45_000 },
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://localhost:4173",
    headless: true,
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "npm run preview",
    url: "http://localhost:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
