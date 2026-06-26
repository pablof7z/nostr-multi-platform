// nsec input hygiene (#2038 item D) — string validation only, NO crypto.
//
// This performs a cheap, surface-level format check on a pasted secret key so
// the UI can reject obvious typos before handing the string to the Rust
// LocalKey provider. It does NOT bech32-decode the key, derive a pubkey, or
// touch any key bytes — the Rust provider is the sole authority on validity and
// the only place the secret is ever interpreted (Chirp thin-shell rule:
// zero crypto in TS).

/** bech32 data charset (BIP-173) — used only to spot non-bech32 characters in a
 *  pasted string. Not a decoder. */
const BECH32_CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";

export type NsecCheck =
  | { ok: true; value: string }
  | { ok: false; reason: string };

/** Surface-level sanity check on a pasted nsec. Trims surrounding whitespace.
 *  Accepts only a lowercase `nsec1…` bech32-shaped string of plausible length.
 *  The Rust provider performs the real decode and reports the authoritative
 *  error if this check passes but the key is still invalid. */
export function checkNsecFormat(raw: string): NsecCheck {
  const value = raw.trim();
  if (value.length === 0) {
    return { ok: false, reason: "Enter your nsec secret key" };
  }
  if (!value.startsWith("nsec1")) {
    return { ok: false, reason: "A secret key must start with “nsec1”" };
  }
  // A bech32-encoded 32-byte secret is 63 chars (nsec1 + 58 data/checksum).
  // Allow a small tolerance rather than asserting an exact length here — the
  // Rust decoder owns the precise contract.
  if (value.length < 60 || value.length > 90) {
    return { ok: false, reason: "That doesn’t look like a complete nsec key" };
  }
  const data = value.slice("nsec1".length);
  for (const ch of data) {
    if (!BECH32_CHARSET.includes(ch)) {
      return { ok: false, reason: "The key contains characters that aren’t valid in an nsec" };
    }
  }
  return { ok: true, value };
}
