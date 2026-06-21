import { defineConfig } from "vitest/config";
import solid from "vite-plugin-solid";

// Components and runtime are shared via the @nmp/components-web and
// @nmp/runtime-web workspace packages (web/packages/*). Those packages are
// source-only (no build step); Vite processes them through the solid plugin
// directly. Excluding them from optimizeDeps ensures the Solid transform runs
// on the package sources rather than esbuild pre-bundling them without it.
export default defineConfig({
  plugins: [solid()],
  build: {
    target: "es2020",
  },
  optimizeDeps: {
    exclude: ["@nmp/components-web", "@nmp/runtime-web"],
  },
  test: {
    environment: "node",
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    passWithNoTests: true,
  },
});
