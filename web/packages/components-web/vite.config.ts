import { resolve } from "node:path";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

const entries = {
  index: resolve(__dirname, "src/index.ts"),
  "component-host/index": resolve(__dirname, "src/component-host/index.ts"),
  "content-core/index": resolve(__dirname, "src/content-core/index.ts"),
  "content-kind-30023/index": resolve(__dirname, "src/content-kind-30023/index.ts"),
  "content-kind-9802/index": resolve(__dirname, "src/content-kind-9802/index.ts"),
  "content-kind-registry/index": resolve(__dirname, "src/content-kind-registry/index.ts"),
  "content-media-grid/index": resolve(__dirname, "src/content-media-grid/index.ts"),
  "content-mention-chip/index": resolve(__dirname, "src/content-mention-chip/index.ts"),
  "content-minimal/index": resolve(__dirname, "src/content-minimal/index.ts"),
  "content-quote-card/index": resolve(__dirname, "src/content-quote-card/index.ts"),
  "content-view/index": resolve(__dirname, "src/content-view/index.ts"),
  "login-block/index": resolve(__dirname, "src/login-block/index.ts"),
  "relay-list/index": resolve(__dirname, "src/relay-list/index.ts"),
  "user-avatar/index": resolve(__dirname, "src/user-avatar/index.ts"),
  "user-card/index": resolve(__dirname, "src/user-card/index.ts"),
  "user-name/index": resolve(__dirname, "src/user-name/index.ts"),
  "user-nip05/index": resolve(__dirname, "src/user-nip05/index.ts"),
  "user-npub/index": resolve(__dirname, "src/user-npub/index.ts"),
};

export default defineConfig({
  plugins: [solid()],
  build: {
    target: "es2020",
    outDir: "dist",
    emptyOutDir: true,
    lib: {
      entry: entries,
      formats: ["es"],
    },
    rollupOptions: {
      external: (id) => id === "flatbuffers" || id === "solid-js" || id.startsWith("solid-js/"),
      output: {
        entryFileNames: "[name].js",
        preserveModules: true,
        preserveModulesRoot: "src",
      },
    },
  },
});
