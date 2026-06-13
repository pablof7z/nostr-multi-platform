// Chirp web composition-root relay policy.
//
// Relay defaults are HOST policy, not framework policy (#1125): the nmp-wasm
// worker protocol no longer carries built-in relay defaults, so the Chirp web
// host supplies its own `relays` + `relay_bootstrap` explicitly in the Start
// request. These values mirror the Rust source of truth in
// `apps/chirp/nmp-chirp-config/src/lib.rs` (CHIRP_RELAY_BOOTSTRAP). Keep the
// two in sync.

export const CHIRP_CONTENT_RELAY_URL = "wss://relay.primal.net";
export const CHIRP_INDEXER_RELAY_URL = "wss://purplepag.es";

export type ChirpRelayBootstrapEntry = { url: string; role: string };

export const CHIRP_RELAY_BOOTSTRAP: ChirpRelayBootstrapEntry[] = [
  { url: CHIRP_CONTENT_RELAY_URL, role: "both,indexer" },
  { url: CHIRP_INDEXER_RELAY_URL, role: "indexer" },
];

export function chirpDefaultRelayUrls(): string[] {
  return CHIRP_RELAY_BOOTSTRAP.map((entry) => entry.url);
}
