import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  build: {
    target: "es2020",
  },
  // @nmp/components-web is source-only (no build step); exclude from esbuild
  // pre-bundling so ?raw imports resolve correctly against the package sources.
  optimizeDeps: {
    exclude: ["@nmp/components-web"],
  },
});
