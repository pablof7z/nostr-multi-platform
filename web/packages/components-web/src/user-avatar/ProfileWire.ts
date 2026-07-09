// Wire type for a Nostr user profile, mirroring the kernel's `ProfileCard`
// row of the `refs.profile` keyed projection (ADR-0070). All fields are raw protocol data as
// produced by the kernel from kind:0 — display names are verbatim, the pubkey
// is 64-char lowercase hex (ADR-0072). The host app owns fetching and
// persistence; these components only render the snapshot they are given.
//
// #3098 — the kernel/wasm boundary must NEVER bake a bech32 npub or its
// abbreviation into this wire (ADR-0072, aim.md §6.9): bech32 ENCODING stays
// Rust-owned (see `nmp_encode_npub` in `@nmpis/runtime-web`, called on demand
// — never reimplemented in JS), but the `npubShort` ABBREVIATION is pure
// string truncation of an already-known npub and the host derives it locally
// via `truncateNpub` below. `npubShort` on this interface is populated by the
// host from that on-demand call, not by the kernel snapshot itself.
export interface ProfileWire {
  /** 64-char lowercase hex pubkey. */
  pubkey: string;
  /** kind:0 display name, verbatim. Absent until a kind:0 arrives. */
  displayName?: string;
  /** kind:0 about/bio text. */
  about?: string;
  /** kind:0 picture URL. Absent or empty when the profile has no picture. */
  pictureUrl?: string;
  /** kind:0 nip05 identifier (e.g. `name@domain`). */
  nip05?: string;
  /** Lightning address / LNURL from kind:0. */
  lnurl?: string;
  /** Locally-truncated npub (e.g. `npub1abcd…wxyz`), derived by the host via
   *  `truncateNpub` from a Rust-encoded npub obtained on demand. Display
   *  only. Absent until the host has resolved an npub for this pubkey. */
  npubShort?: string;
}

/** Parsed avatar URL; `undefined` when no picture is set or the URL is empty. */
export function avatarUrl(profile: ProfileWire | undefined): string | undefined {
  const url = profile?.pictureUrl;
  if (!url || url.length === 0) return undefined;
  return url;
}

/** Stable display label: `displayName` if set, else `npubShort`, else a short
 *  raw-hex form of the pubkey. The hex fallback is honest raw protocol data —
 *  not a locally-derived npub. */
export function displayLabel(profile: ProfileWire | undefined, pubkey: string): string {
  const name = profile?.displayName;
  if (name && name.length > 0) return name;
  if (profile?.npubShort && profile.npubShort.length > 0) return profile.npubShort;
  return shortHex(pubkey);
}

/** `abcd…wxyz` from a 64-char hex pubkey. */
export function shortHex(pubkey: string): string {
  if (pubkey.length <= 12) return pubkey;
  return `${pubkey.slice(0, 6)}…${pubkey.slice(-4)}`;
}

/** Truncate a bech32 npub for display: first 10 chars + `"…"` + last 6
 *  chars, unchanged when already short enough to fit (17 chars or fewer).
 *  Pure string truncation — never re-derives the bech32 encoding itself
 *  (that stays Rust-owned via `nmp_encode_npub`); mirrors the shape
 *  `nmp_core::display::short_npub` used to bake into kernel wires before
 *  #3098 removed that bake. */
export function truncateNpub(npub: string): string {
  if (npub.length <= 17) return npub;
  return `${npub.slice(0, 10)}…${npub.slice(-6)}`;
}
