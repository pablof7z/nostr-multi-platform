import references from "./showcase-references.json";

// Canonical real-data references for the gallery showcase. Mirrors the file
// embedded by the native galleries (apps/nmp-gallery/showcase-references.json)
// — one source of truth for the showcase pubkey, real event refs, and relays.
// Every component renders against THIS real identity resolved by the real
// kernel from real relays. No mocks, no fixtures.

export const showcase = references;

export const SHOWCASE_PUBKEY: string = references.profile.pubkey_hex;

export const SHOWCASE_RELAYS: { url: string; role: string }[] = references.relays.map((r) => ({
  url: r.url,
  // Use indexer-capable roles so profile-claim (kind:0) discovery REQs are
  // routed; `both` alone excludes the indexer lane.
  role: r.role.includes("indexer") ? r.role : `${r.role},indexer`,
}));
