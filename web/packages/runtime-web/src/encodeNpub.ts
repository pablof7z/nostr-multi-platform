// Hex pubkey → { npub, npubShort } via the canonical Rust NIP-19 encoder
// (`nmp_encode_npub`) exposed as a wasm free function.
//
// The wasm module instance lives inside the web worker (wasmBridge.ts). The
// main thread cannot reach the worker's wasm instance directly. This utility
// loads the same wasm module independently on the main thread — a lazy,
// memoized load (one load per page lifetime). The trade-off is documented:
// the wasm binary is already cached by the browser after the worker loads it,
// so the second load reads from the HTTP cache and costs only JS initialisation
// (~1 ms). Never use JS bech32 here — this is the ONLY correct encoding path
// (aim.md §6.9).
//
// The wasm module path must match the path the worker loads — both point at
// the wasm composition root (see #2038) output.

const defaultModulePath = "/nmp-wasm/nmp-browser-runtime.js";

type EncodeNpubFn = (hex: string) => string | undefined | null;

type NmpWasmModule = {
  default?: (input?: unknown) => Promise<unknown> | unknown;
  nmp_encode_npub?: EncodeNpubFn;
};

let encodeNpubFn: EncodeNpubFn | null | undefined;
let loadPromise: Promise<void> | undefined;

async function ensureLoaded(modulePath = defaultModulePath): Promise<void> {
  if (encodeNpubFn !== undefined) return;
  if (loadPromise) {
    await loadPromise;
    return;
  }
  loadPromise = (async () => {
    try {
      const moduleUrl = new URL(modulePath, globalThis.location?.origin ?? "http://localhost").toString();
      const wasmModule = (await import(/* @vite-ignore */ moduleUrl)) as NmpWasmModule;
      if (typeof wasmModule.default === "function") {
        await wasmModule.default();
      }
      encodeNpubFn = wasmModule.nmp_encode_npub ?? null;
    } catch {
      encodeNpubFn = null;
    }
  })();
  await loadPromise;
}

/** Encode a hex pubkey to `{ npub, npubShort }` via the canonical Rust
 *  NIP-19 encoder. Returns `undefined` when the wasm module is unavailable
 *  or the pubkey is invalid (D6 — honest empty, no throw).
 *
 *  Call site: `const result = await encodeNpub(pubkey)`.
 *  The result is stable across calls for the same pubkey — cache it at the
 *  call site if you need a reactive/synchronous read. */
export async function encodeNpub(pubkey: string): Promise<{ npub: string; npubShort: string } | undefined> {
  await ensureLoaded();
  if (!encodeNpubFn) return undefined;
  try {
    const json = encodeNpubFn(pubkey);
    if (!json) return undefined;
    const parsed = JSON.parse(json) as { npub?: string; npubShort?: string };
    if (!parsed.npub || !parsed.npubShort) return undefined;
    return { npub: parsed.npub, npubShort: parsed.npubShort };
  } catch {
    return undefined;
  }
}
