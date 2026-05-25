import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// The site embeds Swift source files from `crates/nmp-cli/registry/...` via
// `?raw` imports. Vite's default `server.fs.allow` is the project root only,
// so we extend it up to the workspace root (`../..`) for dev mode. Production
// `vite build` inlines the strings at compile time, so the workspace path
// never leaks into shipped output.
export default defineConfig({
  plugins: [solid()],
  build: {
    target: "es2020",
  },
  server: {
    fs: {
      allow: ["../.."],
    },
  },
});
