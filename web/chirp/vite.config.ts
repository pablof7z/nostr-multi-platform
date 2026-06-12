import { defineConfig } from "vitest/config";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  build: {
    target: "es2020",
  },
  test: {
    environment: "node",
    // Only pick up unit tests from src/ — exclude the Playwright E2E tests
    // in tests/ which use a different runner (@playwright/test) and conflict
    // with Vitest's test() API when accidentally imported.
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
