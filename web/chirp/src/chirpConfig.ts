// Chirp web composition-root relay policy — single-sourced from the Rust crate.
//
// Relay defaults are Chirp app/operator policy, not framework policy
// (#1125/#1493). The authoritative values live in
// apps/chirp/crates/nmp-chirp-config/src/lib.rs; chirpConfig.generated.ts is
// produced from them by scripts/gen-chirp-config.mjs so the web host can never
// drift (#1546 F6). Run `npm run codegen:chirp-config -w @nmp/chirp-web` to
// regenerate.
//
// This module re-exports from the generated file and adds the override logic used
// by tests and the `?relay_bootstrap=` URL parameter.
export {
  CHIRP_CONTENT_RELAY_URL,
  CHIRP_INDEXER_RELAY_URL,
  CHIRP_SEARCH_RELAY_URL,
  CHIRP_PUBLIC_GROUP_RELAY_URL,
  CHIRP_RELAY_BOOTSTRAP,
  chirpDefaultRelayUrls,
} from "./chirpConfig.generated";
import {
  CHIRP_RELAY_BOOTSTRAP as GENERATED_RELAY_BOOTSTRAP,
  chirpDefaultRelayUrls as generatedDefaultRelayUrls,
  type ChirpRelayBootstrapEntry,
} from "./chirpConfig.generated";

export type { ChirpRelayBootstrapEntry } from "./chirpConfig.generated";

export type ChirpRelayStartOverride = string[] | ChirpRelayBootstrapEntry[];

function isBootstrapEntry(value: unknown): value is ChirpRelayBootstrapEntry {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return typeof candidate.url === "string" && typeof candidate.role === "string";
}

function bootstrapFromTuple(value: unknown): ChirpRelayBootstrapEntry | undefined {
  if (!Array.isArray(value) || value.length !== 2) return undefined;
  const [url, role] = value;
  if (typeof url !== "string" || typeof role !== "string") return undefined;
  return { url, role };
}

function parseRelayBootstrapParam(value: string | null): ChirpRelayBootstrapEntry[] | undefined {
  if (!value) return undefined;
  try {
    const parsed = JSON.parse(value) as unknown;
    if (!Array.isArray(parsed)) return undefined;
    const entries = parsed
      .map((item) => (isBootstrapEntry(item) ? item : bootstrapFromTuple(item)))
      .filter((entry): entry is ChirpRelayBootstrapEntry => entry !== undefined);
    return entries.length === parsed.length && entries.length > 0 ? entries : undefined;
  } catch {
    return undefined;
  }
}

/** Resolve the `relays` + `relay_bootstrap` the Chirp web host supplies in the
 *  Start request. Relay policy is Chirp app/operator policy (#1125/#1493):
 *  the browser worker protocol has no built-in defaults, so the host always sends an
 *  explicit list.
 *
 *  Tests and local dev may pass role-explicit relay entries to preserve the
 *  same outbox topology used by iOS/Android: indexer-only relays remain
 *  discovery relays, while write-capable relays seed the local publish lane. */
export function chirpStartRelays(overrideRelays?: ChirpRelayStartOverride): {
  relays: string[];
  relay_bootstrap: ChirpRelayBootstrapEntry[];
} {
  if (overrideRelays && overrideRelays.length > 0) {
    if (isBootstrapEntry(overrideRelays[0])) {
      const relay_bootstrap = overrideRelays as ChirpRelayBootstrapEntry[];
      return {
        relays: relay_bootstrap.map((entry) => entry.url),
        relay_bootstrap,
      };
    }
    const relays = overrideRelays as string[];
    return {
      relays,
      relay_bootstrap: relays.map((url) => ({ url, role: "both,indexer" })),
    };
  }
  return {
    relays: generatedDefaultRelayUrls(),
    relay_bootstrap: GENERATED_RELAY_BOOTSTRAP,
  };
}

export function chirpRelayOverrideFromSearch(search: string): ChirpRelayStartOverride | undefined {
  const params = new URLSearchParams(search);
  const explicitBootstrap =
    parseRelayBootstrapParam(params.get("relay_bootstrap"))
    ?? parseRelayBootstrapParam(params.get("relayBootstrap"));
  if (explicitBootstrap) return explicitBootstrap;
  const relays = params.getAll("relay").filter(Boolean);
  return relays.length > 0 ? relays : undefined;
}

export function chirpSearchRelayUrlsFromSearch(search: string): string[] | undefined {
  const params = new URLSearchParams(search);
  const relays = params.getAll("search_relay").concat(params.getAll("searchRelay")).filter(Boolean);
  return relays.length > 0 ? relays : undefined;
}

export function chirpGroupRelayUrlFromSearch(search: string): string | undefined {
  const params = new URLSearchParams(search);
  return params.get("group_relay") ?? params.get("groupRelay") ?? undefined;
}
