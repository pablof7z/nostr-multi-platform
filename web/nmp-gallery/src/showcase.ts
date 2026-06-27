import references from "./showcase-references.json";

// Canonical real-data references for the gallery showcase. Mirrors the file
// embedded by the native galleries (apps/nmp-gallery/showcase-references.json)
// — one source of truth for the showcase pubkey, real event refs, and relays.
// Every component renders against THIS real identity resolved by the real
// kernel from real relays. No mocks, no fixtures.

export const showcase = references;

export const SHOWCASE_PUBKEY: string = references.profile.pubkey_hex;

// Real events to showcase the content-view component. `uri` is the `nostr:`
// address the kernel resolves via the content-relay lane; `primary_id` is the
// key the resolved event appears under in the `refs.event` projection.
export const SHOWCASE_NOTE = {
  uri: references.note.uri,
  primaryId: references.note.primary_id,
  relayHints: references.note.relay_hints,
};
export const SHOWCASE_ARTICLE = {
  uri: references.article.uri,
  primaryId: references.article.primary_id,
  relayHints: references.article.relay_hints,
};
export const SHOWCASE_HIGHLIGHT = {
  uri: references.highlight.uri,
  primaryId: references.highlight.primary_id,
  relayHints: references.highlight.relay_hints,
};

// Content relays where the showcase EVENTS actually live. Both the note's
// `nevent` and the article's `naddr` embed `wss://nos.lol` as their relay hint
// (NIP-19 TLV type 1) — i.e. the events themselves point here. We must declare
// these up-front because the WASM transport, unlike native, does NOT dial relay
// hints on demand: `crates/nmp-wasm/src/relay_pool.rs` spawns one WebSocket per
// bootstrap entry at Start and silently drops any outbound REQ to a relay it has
// no driver for (a deliberate "host declares its relay policy up-front" design).
// Native's `actor::relay_mgmt::send_outbound` spawns workers on demand; the web
// transport has no equivalent. Declaring the hinted content relay is honoring
// the event's own routing, not a workaround — the content still resolves from a
// real relay, parsed by the real kernel. The capability gap itself is tracked
// separately (see WIP.md / wasm-on-demand-relay-dial memory).
const CONTENT_RELAYS: { url: string; role: string }[] = [
  { url: "wss://nos.lol", role: "both" },
  // Redundant content relay: the showcase note + article are also on damus, so a
  // cold claim still resolves if one relay is slow/unreachable. (The highlight
  // lives only on nos.lol.)
  { url: "wss://relay.damus.io", role: "both" },
];

export const SHOWCASE_RELAYS: { url: string; role: string }[] = [
  ...references.relays.map((r) => ({
    url: r.url,
    // Use indexer-capable roles so profile-claim (kind:0) discovery REQs are
    // routed; `both` alone excludes the indexer lane.
    role: r.role.includes("indexer") ? r.role : `${r.role},indexer`,
  })),
  ...CONTENT_RELAYS,
];
