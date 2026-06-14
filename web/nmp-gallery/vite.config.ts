import { defineConfig } from "vitest/config";
import solid from "vite-plugin-solid";

// The user-* components are vendored byte-identical from the registry
// (web/registry/src/vendor/web/<component>) into src/components — see
// scripts/check-component-drift.sh, which a vitest test enforces. The gallery
// is self-contained so it deploys independently (nmp-gallery.f7z.io).
export default defineConfig({
  plugins: [solid()],
  build: {
    target: "es2020",
  },
  test: {
    environment: "node",
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
