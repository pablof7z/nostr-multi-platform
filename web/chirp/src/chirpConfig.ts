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
  // role MUST match nmp-chirp-config::CHIRP_RELAY_BOOTSTRAP exactly. The
  // content relay is "both" (read/write); purplepag.es is the dedicated
  // indexer. (#1493 fixed a drift where this said "both,indexer" — Primal is
  // not also an indexer; discovery/outbox lookups go to purplepag.es.)
  // FOLLOW-UP: generate this list from the Rust source so it cannot re-drift.
  { url: CHIRP_CONTENT_RELAY_URL, role: "both" },
  { url: CHIRP_INDEXER_RELAY_URL, role: "indexer" },
];

export function chirpDefaultRelayUrls(): string[] {
  return CHIRP_RELAY_BOOTSTRAP.map((entry) => entry.url);
}
