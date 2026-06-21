// Pure-TS NIP-19 npub encoder. Replaces the nmp_encode_npub worker message —
// bech32 encoding is stateless and needs no actor, so the WASM kernel must
// NOT be on the hot path for it. Call encodeNpub() directly on the main thread.
//
// aim.md §6.9: npubs are always Rust-formatted (canonical NIP-19). This module
// produces byte-identical output to the Rust bech32 encoder for valid 32-byte
// pubkeys using the same BIP-0173 bech32 algorithm.

const CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const GENERATOR = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];

function polymod(values: number[]): number {
  let chk = 1;
  for (const v of values) {
    const top = chk >> 25;
    chk = ((chk & 0x1ffffff) << 5) ^ v;
    for (let i = 0; i < 5; i++) {
      if ((top >> i) & 1) chk ^= GENERATOR[i];
    }
  }
  return chk;
}

function hrpExpand(hrp: string): number[] {
  const ret: number[] = [];
  for (let i = 0; i < hrp.length; i++) ret.push(hrp.charCodeAt(i) >> 5);
  ret.push(0);
  for (let i = 0; i < hrp.length; i++) ret.push(hrp.charCodeAt(i) & 31);
  return ret;
}

function createChecksum(hrp: string, data: number[]): number[] {
  const values = hrpExpand(hrp).concat(data).concat([0, 0, 0, 0, 0, 0]);
  const mod = polymod(values) ^ 1;
  const ret: number[] = [];
  for (let i = 0; i < 6; i++) ret.push((mod >> (5 * (5 - i))) & 31);
  return ret;
}

function convertBits(data: number[], frombits: number, tobits: number, pad: boolean): number[] | null {
  let acc = 0;
  let bits = 0;
  const ret: number[] = [];
  const maxv = (1 << tobits) - 1;
  for (const value of data) {
    if (value < 0 || value >> frombits !== 0) return null;
    acc = (acc << frombits) | value;
    bits += frombits;
    while (bits >= tobits) {
      bits -= tobits;
      ret.push((acc >> bits) & maxv);
    }
  }
  if (pad) {
    if (bits > 0) ret.push((acc << (tobits - bits)) & maxv);
  } else if (bits >= frombits || ((acc << (tobits - bits)) & maxv) !== 0) {
    return null;
  }
  return ret;
}

function bech32Encode(hrp: string, data: number[]): string {
  const combined = data.concat(createChecksum(hrp, data));
  return hrp + "1" + combined.map((d) => CHARSET[d]).join("");
}

export interface NpubResult {
  /** Full bech32 npub, e.g. `npub1abc...xyz`. */
  npub: string;
  /** Truncated form for display, e.g. `npub1abc…wxyz`. */
  npubShort: string;
}

/** Encode a 64-char lowercase hex pubkey to its NIP-19 bech32 npub form.
 *  Returns null for invalid input (wrong length, non-hex). D6 compliant:
 *  callers fall back to the raw hex rather than deriving npub locally. */
export function encodeNpub(pubkeyHex: string): NpubResult | null {
  if (pubkeyHex.length !== 64) return null;
  try {
    const bytes: number[] = [];
    for (let i = 0; i < 64; i += 2) {
      const byte = parseInt(pubkeyHex.slice(i, i + 2), 16);
      if (isNaN(byte)) return null;
      bytes.push(byte);
    }
    const words = convertBits(bytes, 8, 5, true);
    if (!words) return null;
    const npub = bech32Encode("npub", words);
    // Truncated: first 12 chars + ellipsis + last 8 chars (matches Rust format)
    const npubShort = npub.slice(0, 12) + "…" + npub.slice(-8);
    return { npub, npubShort };
  } catch {
    return null;
  }
}
