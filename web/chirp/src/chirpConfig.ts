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

/** Resolve the `relays` + `relay_bootstrap` the Chirp web host supplies in the
 *  Start request. Relay policy is host policy (#1125): the nmp-wasm protocol
 *  has no built-in defaults, so the host always sends an explicit list.
 *
 *  When `overrideRelays` is supplied (e.g. the Playwright smoke test via the
 *  `?relay=` query parameter), those URLs replace the Chirp defaults. Each is
 *  given role "both,indexer" (not just "both") so a single injected relay also
 *  serves profile-claim discovery requests (BootstrapSeed::IndexerOnly) —
 *  "both" alone excludes the indexer lane, so a relay supplied via ?relay=
 *  would otherwise silently receive no kind:0 claim REQs. Otherwise the host
 *  sends its own Chirp relay defaults (mirrors nmp-chirp-config). */
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
