#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACK_DIR="$(mktemp -d)"
APP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$PACK_DIR" "$APP_DIR"
}
trap cleanup EXIT

runtime_version="$(node -p "require(process.argv[1]).version" "$ROOT/web/packages/runtime-web/package.json")"
components_version="$(node -p "require(process.argv[1]).version" "$ROOT/web/packages/components-web/package.json")"
runtime_tarball="$PACK_DIR/nmp-runtime-web-${runtime_version}.tgz"
components_tarball="$PACK_DIR/nmp-components-web-${components_version}.tgz"

npm --prefix "$ROOT/web" ci
npm --prefix "$ROOT/web" pack --workspace @nmp/runtime-web --pack-destination "$PACK_DIR"
npm --prefix "$ROOT/web" pack --workspace @nmp/components-web --pack-destination "$PACK_DIR"

if [[ ! -f "$runtime_tarball" ]]; then
  echo "missing runtime-web tarball: $runtime_tarball" >&2
  exit 1
fi

if [[ ! -f "$components_tarball" ]]; then
  echo "missing components-web tarball: $components_tarball" >&2
  exit 1
fi

require_tarball_entry() {
  local tarball="$1"
  local entry="$2"
  local entries_file="$PACK_DIR/$(basename "$tarball").entries"
  tar -tzf "$tarball" > "$entries_file"
  if ! grep -Fxq "$entry" "$entries_file"; then
    echo "tarball $tarball missing required entry: $entry" >&2
    exit 1
  fi
}

require_tarball_entry "$runtime_tarball" "package/dist/wasm/nmp_browser_runtime.js"
require_tarball_entry "$runtime_tarball" "package/dist/wasm/nmp_browser_runtime_bg.wasm"
require_tarball_entry "$components_tarball" "package/dist/index.js"
require_tarball_entry "$components_tarball" "package/dist/component-host/index.js"

mkdir -p "$APP_DIR/src"

cat > "$APP_DIR/package.json" <<'JSON'
{
  "name": "nmp-web-package-consumer-smoke",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "scripts": {
    "build": "vite build",
    "typecheck": "tsc --noEmit"
  },
  "dependencies": {
    "flatbuffers": "^25.9.23",
    "solid-js": "^1.9.13"
  },
  "devDependencies": {
    "@types/node": "^25.9.3",
    "typescript": "^6.0.3",
    "vite": "^8.0.14",
    "vite-plugin-solid": "^2.11.12"
  }
}
JSON

cat > "$APP_DIR/index.html" <<'HTML'
<div id="root"></div>
<script type="module" src="/src/main.tsx"></script>
HTML

cat > "$APP_DIR/tsconfig.json" <<'JSON'
{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "strict": true,
    "skipLibCheck": true,
    "jsx": "preserve",
    "jsxImportSource": "solid-js",
    "types": ["node", "vite/client"]
  },
  "include": ["src", "vite.config.ts"]
}
JSON

cat > "$APP_DIR/vite.config.ts" <<'TS'
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  build: {
    target: "es2020",
  },
});
TS

cat > "$APP_DIR/src/main.tsx" <<'TSX'
import {
  DegradedRuntime,
  GeneratedActionBuilders,
  protocolVersion,
  type WorkerEvent,
  type WorkerRequest,
} from "@nmp/runtime-web";
import {
  NmpComponentHostProvider,
  NostrAvatar,
  type EventRefResolver,
  type NostrProfileHost,
  type ProfileWire,
  type ResolvedEventEmbeds,
} from "@nmp/components-web";
import { NostrProfileName } from "@nmp/components-web/user-name";

const workerUrl = new URL("@nmp/runtime-web/worker", import.meta.url);
const maybeWorker = typeof Worker === "undefined" ? undefined : new Worker(workerUrl, { type: "module" });

const profile: ProfileWire = {
  pubkey: "0".repeat(64),
  displayName: "Alice",
};

const host: NostrProfileHost = {
  profile: () => profile,
  resolveProfileRef: () => {},
  releaseProfileRef: () => {},
};

const resolver: EventRefResolver = {
  resolveEventRef: () => {},
  releaseEventRef: () => {},
};

const embeds = (): ResolvedEventEmbeds => new Map();
const request: WorkerRequest = {
  type: "hello",
  app_id: "smoke",
  platform: "web",
  protocol_version: protocolVersion,
};
const runtime = new DegradedRuntime("browser_bridge_unavailable", "smoke");
const events: WorkerEvent[] = runtime.handle(request);

void maybeWorker;
void GeneratedActionBuilders;
void events;

export function SmokeApp() {
  return (
    <NmpComponentHostProvider
      profileHost={host}
      resolvedEventEmbeds={embeds}
      eventRefResolver={resolver}
    >
      <NostrAvatar pubkey={profile.pubkey} size={32} />
      <NostrProfileName profile={profile} />
    </NmpComponentHostProvider>
  );
}
TSX

npm --prefix "$APP_DIR" install --ignore-scripts
npm --prefix "$APP_DIR" install --ignore-scripts "$runtime_tarball" "$components_tarball"
npm --prefix "$APP_DIR" run typecheck
npm --prefix "$APP_DIR" run build

echo "web package consumer smoke ok"
