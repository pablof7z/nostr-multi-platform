// GENERATED — do not edit by hand. Run `pnpm codegen:chirp-config` to regenerate.
// Source: apps/chirp/nmp-chirp-config/src/lib.rs (CHIRP_*_URL constants + CHIRP_RELAY_BOOTSTRAP).
//
// Relay defaults are HOST policy, not framework policy (#1125): the nmp-wasm
// worker protocol carries no built-in relay defaults, so the Chirp web host
// supplies its own `relays` + `relay_bootstrap` in the Start request. These are
// single-sourced from the Rust crate so the two can never drift (#1546 F6).

export const CHIRP_CONTENT_RELAY_URL = "wss://relay.primal.net";
export const CHIRP_INDEXER_RELAY_URL = "wss://purplepag.es";
export const CHIRP_SEARCH_RELAY_URL = "wss://relay.nostr.band";
export const CHIRP_PUBLIC_GROUP_RELAY_URL = "wss://relay.groups.nip29.com";

export type ChirpRelayBootstrapEntry = { url: string; role: string };

export const CHIRP_RELAY_BOOTSTRAP: ChirpRelayBootstrapEntry[] = [
  { url: CHIRP_CONTENT_RELAY_URL, role: "both" },
  { url: CHIRP_INDEXER_RELAY_URL, role: "indexer" },
];

export function chirpDefaultRelayUrls(): string[] {
  return CHIRP_RELAY_BOOTSTRAP.map((entry) => entry.url);
}

/** Resolve the `relays` + `relay_bootstrap` the Chirp web host supplies in the
 *  Start request. Relay policy is host policy (#1125): the nmp-wasm protocol
 *  has no built-in defaults, so the host always sends an explicit list.
 *
 *  When `overrideRelays` is supplied (e.g. the Playwright smoke test via the
 *  `?relay=` query parameter), those URLs replace the Chirp defaults. Each is
 *  given role "both,indexer" (not just "both") so a single injected relay also
 *  serves profile-claim discovery requests (BootstrapSeed::IndexerOnly). */
export function chirpStartRelays(overrideRelays?: string[]): {
  relays: string[];
  relay_bootstrap: ChirpRelayBootstrapEntry[];
} {
  if (overrideRelays && overrideRelays.length > 0) {
    return {
      relays: overrideRelays,
      relay_bootstrap: overrideRelays.map((url) => ({ url, role: "both,indexer" })),
    };
  }
  return {
    relays: chirpDefaultRelayUrls(),
    relay_bootstrap: CHIRP_RELAY_BOOTSTRAP,
  };
}
