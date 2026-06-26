// NIP-07 browser-extension adapter (#2038 item D).
//
// Thin wrapper over `window.nostr` (the EIP-1193-style Nostr extension). This is
// the ONLY surface in Chirp Web that touches the extension's crypto — and it
// only ever asks the extension for the public key and relay list. Event signing
// is delegated to the extension by the main-thread broker (signBroker.ts); no
// key material is ever decoded or signed-with in TS (Chirp thin-shell rule).
//
// `window.nostr` is declared globally in App.tsx (shared with signBroker.ts and
// client.ts). This module relies on that ambient declaration.

import type { IdentityRelayPermission } from "@nmp/runtime-web";

/** True when a NIP-07 extension exposes `window.nostr`. */
export function hasNip07Extension(): boolean {
  return typeof window !== "undefined" && Boolean(window.nostr);
}

/** Fetch the active account's hex pubkey from the extension.
 *  Throws if no extension is present or the user rejects. */
export async function nip07PublicKey(): Promise<string> {
  if (!window.nostr) {
    throw new Error("no NIP-07 extension (window.nostr is undefined)");
  }
  return window.nostr.getPublicKey();
}

/** Map a raw NIP-07 `getRelays()` result into the runtime's relay-permission
 *  shape. Canonicalization + role mapping is owned by the Rust worker; this only
 *  reshapes the extension's `{url: {read, write}}` map into a typed array. */
export function identityRelaysFromNip07(value: unknown): IdentityRelayPermission[] {
  if (typeof value !== "object" || value === null) return [];
  const relays: IdentityRelayPermission[] = [];
  for (const [url, permissions] of Object.entries(value)) {
    if (typeof url !== "string" || typeof permissions !== "object" || permissions === null) {
      continue;
    }
    const raw = permissions as Record<string, unknown>;
    relays.push({ url, read: raw.read === true, write: raw.write === true });
  }
  return relays;
}

/** Read the extension's relay list, if it exposes `getRelays()`. Returns
 *  `undefined` (not an error) when unsupported or empty so the caller can let
 *  the runtime fall back to host relay policy. */
export async function readNip07Relays(): Promise<IdentityRelayPermission[] | undefined> {
  if (!window.nostr?.getRelays) return undefined;
  try {
    const relays = identityRelaysFromNip07(await window.nostr.getRelays());
    return relays.length > 0 ? relays : undefined;
  } catch {
    return undefined;
  }
}
