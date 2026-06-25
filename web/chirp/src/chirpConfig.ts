// Chirp web composition-root relay policy — single-sourced from the Rust crate.
//
// Relay defaults are HOST policy, not framework policy (#1125). The authoritative
// values live in apps/chirp/crates/nmp-chirp-config/src/lib.rs; chirpConfig.generated.ts
// is produced from them by scripts/gen-chirp-config.mjs so the web host can never
// drift (#1546 F6). Run `pnpm codegen:chirp-config` to regenerate.
//
// This module is a re-export shim so existing import paths keep working.
export {
  CHIRP_CONTENT_RELAY_URL,
  CHIRP_INDEXER_RELAY_URL,
  CHIRP_SEARCH_RELAY_URL,
  CHIRP_PUBLIC_GROUP_RELAY_URL,
  CHIRP_RELAY_BOOTSTRAP,
  chirpDefaultRelayUrls,
  chirpStartRelays,
} from "./chirpConfig.generated";
export type { ChirpRelayBootstrapEntry } from "./chirpConfig.generated";
