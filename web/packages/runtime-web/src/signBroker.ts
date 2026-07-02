import type { WorkerEvent, WorkerRequest } from "./protocol";

// #2722 — the main-thread NIP-07 sign-request round-trip.
//
// Web Workers have no browser-extension surface, so when the wasm worker
// parks a sign op and emits a `sign_request` (see `protocol.ts`'s
// `begin_sign` round-trip), the MAIN THREAD fulfils it — pure message
// re-entry, no polling (D8).
//
// Browser boundary (#2082, enforced by
// `crates/nmp-testing/bin/doctrine-lint/browser_boundary_gates.rs`):
// runtime-web is ABI/Worker glue ONLY — it must never call a browser signer
// extension directly (no literal browser-extension identity/sign calls in
// this package). The HOST supplies a `Nip07Signer` — typically a thin adapter
// over its own extension access — and this module owns only the reusable
// protocol shape: parsing `sign_request`, the account-pin guard, JSON
// parse/round-trip, and posting `deliver_signer_response` on every outcome
// (success, rejection, mismatch, malformed payload) so the worker's parked op
// never hangs.

/** The capability a host injects to fulfil one sign round-trip. Method names
 *  are deliberately NOT the browser-extension API names — this package never
 *  references that API directly; the host's own adapter (e.g. wrapping its
 *  extension's identity/sign calls) supplies this shape. */
export interface Nip07Signer {
  /** The signer's currently active identity (hex pubkey). */
  publicKey(): Promise<string>;
  /** Sign an unsigned Nostr event, returning the signed event. */
  sign(event: Record<string, unknown>): Promise<Record<string, unknown>>;
}

/** One parsed `sign_request` — the round-trip's input. */
export type SignRequest = {
  correlationId: string;
  accountPubkey: string;
  unsignedJson: string;
};

/** Extract a `sign_request` from a decoded `WorkerEvent`, or `undefined` when
 *  `event` is a different event type. Pure data extraction — no signer call. */
export function asSignRequest(event: WorkerEvent): SignRequest | undefined {
  if (event.type !== "sign_request") return undefined;
  return {
    correlationId: event.correlation_id,
    accountPubkey: event.account_pubkey,
    unsignedJson: event.unsigned_json,
  };
}

/** Post the `deliver_signer_response` completing one sign round-trip. Exactly
 *  one of `signedJson` / `error` should be non-null. */
export function deliverSignerResponse(
  post: (request: WorkerRequest) => void,
  correlationId: string,
  signedJson: string | null,
  error: string | null,
): void {
  post({
    type: "deliver_signer_response",
    correlation_id: correlationId,
    signed_json: signedJson,
    error,
  });
}

/**
 * Fulfil one `SignRequest` via a host-supplied `signer`. Every failure mode
 * (account mismatch, malformed unsigned JSON, signer rejection) posts a
 * `deliver_signer_response` with `error` set, so the worker fails the
 * round-trip closed rather than leaving the op parked forever.
 *
 * Account-pin guard: `signer.publicKey()` must match `request.accountPubkey`
 * (case-insensitive) before signing — the round-trip was begun for a specific
 * account, and a signer whose active identity has since changed must not
 * silently sign with the wrong one.
 */
export async function fulfilSignRequest(
  post: (request: WorkerRequest) => void,
  signer: Nip07Signer,
  request: SignRequest,
): Promise<void> {
  const deliver = (signedJson: string | null, error: string | null) =>
    deliverSignerResponse(post, request.correlationId, signedJson, error);

  let activePubkey: string;
  try {
    activePubkey = await signer.publicKey();
  } catch (e) {
    deliver(null, `signer.publicKey() rejected: ${String(e)}`);
    return;
  }
  if (activePubkey.toLowerCase() !== request.accountPubkey.toLowerCase()) {
    deliver(
      null,
      `signer is on a different account (${activePubkey}) than the signing ` +
        `request (${request.accountPubkey}); switch the signer's active account`,
    );
    return;
  }

  let unsigned: Record<string, unknown>;
  try {
    unsigned = JSON.parse(request.unsignedJson) as Record<string, unknown>;
  } catch (e) {
    deliver(null, `unsigned event JSON did not parse: ${String(e)}`);
    return;
  }

  try {
    const signed = await signer.sign(unsigned);
    deliver(JSON.stringify(signed), null);
  } catch (e) {
    deliver(null, `signer.sign() rejected: ${String(e)}`);
  }
}

/**
 * Wire the round-trip onto a live worker + host-supplied `signer`: listens
 * for `sign_request` messages and fulfils each via `fulfilSignRequest`.
 * Returns an unsubscribe function.
 *
 * Sugar over `fulfilSignRequest` for the common one-worker case; a host with a
 * custom message bus should call `asSignRequest` / `fulfilSignRequest`
 * directly instead.
 */
export function installNip07SignBroker(
  worker: {
    addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
    removeEventListener(type: "message", listener: (event: MessageEvent) => void): void;
    postMessage(request: WorkerRequest): void;
  },
  signer: Nip07Signer,
): () => void {
  const listener = (event: MessageEvent) => {
    const data = event.data as WorkerEvent | undefined;
    const request = data ? asSignRequest(data) : undefined;
    if (!request) return;
    void fulfilSignRequest((r) => worker.postMessage(r), signer, request);
  };
  worker.addEventListener("message", listener);
  return () => worker.removeEventListener("message", listener);
}
