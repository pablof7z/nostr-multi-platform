// Wire type for a Nostr user profile, mirroring the kernel's `ProfileCard`
// projection (resolved_profiles / KRPR). All fields are raw protocol data as
// produced by the kernel from kind:0 — display names are verbatim, the pubkey
// is 64-char lowercase hex (ADR-0032). The host app owns fetching and
// persistence; these components only render the snapshot they are given.
//
// `npubShort` is intentionally optional and Rust-formatted when present — never
// reformat or bech32-encode it in the browser (aim.md §6.9). The web kernel
// boundary does not yet emit it; until it does the components fall back to the
// raw pubkey rather than deriving an npub locally.
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
  /** Rust-truncated npub (e.g. `npub1abcd…wxyz`). Display only. Optional until
   *  the web kernel boundary emits it. */
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
