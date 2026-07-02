import type { ProfileSnapshot } from "./generated/nmp/kernel/profile-snapshot";

// ADR-0063 Lane G twin (#2722) — the hand-written glue `keyedRefCache.generated.ts`
// calls to map a `flatc --ts` reader struct to a package domain type. This is
// the web sibling of Chirp's `TypedProjectionGlue.swift` / `KeyedRefDecoders.kt`:
// the generator emits the mechanical envelope lookup + checked decode, this
// file owns the one non-mechanical step (reader field -> domain field).

/** Thin hydrated profile type — the web domain twin of Chirp's `ProfileCard`. */
export type ProfileWire = {
  pubkey: string;
  displayName?: string;
  pictureUrl?: string;
  nip05?: string;
  about?: string;
  lnurl?: string;
};

/**
 * Map a decoded `ProfileSnapshot` reader to a `ProfileWire`. Returns
 * `undefined` when the reader carries no `card` (a well-formed but empty
 * buffer) or no `pubkey` (required identity field) — the generated caller
 * treats that as a decode failure (D6 fail-closed).
 */
export function refRowProfile(reader: ProfileSnapshot): ProfileWire | undefined {
  const card = reader.card();
  if (!card) return undefined;
  const pubkey = card.pubkey();
  if (pubkey === null) return undefined;

  const wire: ProfileWire = { pubkey };
  if (card.hasDisplayName()) {
    const v = card.displayName();
    if (v) wire.displayName = v;
  }
  if (card.hasPictureUrl()) {
    const v = card.pictureUrl();
    if (v) wire.pictureUrl = v;
  }
  const nip05 = card.nip05();
  if (nip05) wire.nip05 = nip05;
  const about = card.about();
  if (about) wire.about = about;
  if (card.hasLnurl()) {
    const v = card.lnurl();
    if (v) wire.lnurl = v;
  }
  return wire;
}
