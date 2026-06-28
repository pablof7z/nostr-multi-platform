// GENERATED — do not edit by hand. Run `npm run codegen:chirp-config -w @nmp/chirp-web` to regenerate.
// Source: apps/chirp/crates/nmp-chirp-config/src/lib.rs (CHIRP_*_URL constants + CHIRP_RELAY_BOOTSTRAP).
//
// Relay defaults are Chirp app/operator policy, not framework policy
// (#1125/#1493): the browser worker protocol carries no built-in relay
// defaults, so the Chirp web host supplies its own `relays` +
// `relay_bootstrap` in the Start request. These are single-sourced from the
// Rust crate so the two can never drift (#1546 F6).

export const CHIRP_CONTENT_RELAY_URL = "wss://relay.primal.net";
export const CHIRP_INDEXER_RELAY_URL = "wss://purplepag.es";
export const CHIRP_SEARCH_RELAY_URL = "wss://relay.nostr.band";
export const CHIRP_PUBLIC_GROUP_RELAY_URL = "wss://relay.groups.nip29.com";

export type ChirpRelayBootstrapEntry = { url: string; role: string };

export const CHIRP_RELAY_BOOTSTRAP: ChirpRelayBootstrapEntry[] = [
  { url: CHIRP_CONTENT_RELAY_URL, role: "both,indexer" },
  { url: CHIRP_INDEXER_RELAY_URL, role: "indexer" },
];

export function chirpDefaultRelayUrls(): string[] {
  return CHIRP_RELAY_BOOTSTRAP.map((entry) => entry.url);
}

/** Resolve the `relays` + `relay_bootstrap` the Chirp web host supplies in the
 *  Start request. Relay policy is Chirp app/operator policy (#1125/#1493):
 *  the browser worker protocol has no built-in defaults, so the host always sends an
 *  explicit list.
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
