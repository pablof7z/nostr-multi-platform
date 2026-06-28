import type { WorkerRequest } from "@nmp/runtime-web";

/** #1753 S6 — the main-thread NIP-07 broker.
 *
 * Web Workers have no `window.nostr`, so the wasm worker emits a `sign_request`
 * and the MAIN THREAD fulfils it here: call `window.nostr.signEvent`, then post
 * the result back as a `deliver_signer_response` so the worker resumes its
 * parked sign op (pure message re-entry — no polling, D8).
 *
 * Scope (#2038 item D): this broker fulfils the NIP-07 (browser-extension) sign
 * path ONLY. The local-key (nsec) path never reaches here — the Rust LocalKey
 * provider signs inside the worker and emits no `sign_request`, so no event is
 * ever signed on the main thread for local-key accounts. The ONLY crypto this
 * file performs is delegating to `window.nostr.signEvent` (the extension signs).
 *
 * `post` is the worker's message sink (`worker.postMessage`). Every failure
 * mode (no extension, account mismatch, malformed event, user rejection) is
 * posted back with `error` set so the worker fails the round-trip closed (D6)
 * rather than leaving the op parked forever. The signature is not published
 * here — the worker routes successful publish continuations through the Rust
 * kernel's `publish_pre_signed` path.
 */
export async function fulfilSignRequestViaExtension(
  post: (request: WorkerRequest) => void,
  correlationId: string,
  unsignedJson: string,
  accountPubkey: string,
): Promise<void> {
  const deliver = (signedJson: string | null, error: string | null) => {
    post({
      type: "deliver_signer_response",
      correlation_id: correlationId,
      signed_json: signedJson,
      error,
    });
  };
  if (!window.nostr) {
    deliver(null, "window.nostr is unavailable — no NIP-07 extension installed");
    return;
  }
  // Account-pin guard (D6): a NIP-07 extension signs with whichever account is
  // currently active. If the active account differs from the one this sign
  // round-trip was begun for, fail early with an actionable message rather than
  // producing a signature the worker will reject (account-pinned, see
  // `deliver_signer_response` in protocol.ts). The worker remains the final
  // authority; this is an early, honest short-circuit.
  try {
    const activePubkey = await window.nostr.getPublicKey();
    if (activePubkey.toLowerCase() !== accountPubkey.toLowerCase()) {
      deliver(
        null,
        `NIP-07 extension is on a different account (${activePubkey}) than the ` +
          `signing request (${accountPubkey}); switch the extension's active account`,
      );
      return;
    }
  } catch (e) {
    deliver(null, `window.nostr.getPublicKey rejected: ${String(e)}`);
    return;
  }
  let unsigned: Record<string, unknown>;
  try {
    unsigned = JSON.parse(unsignedJson) as Record<string, unknown>;
  } catch (e) {
    deliver(null, `unsigned event JSON did not parse: ${String(e)}`);
    return;
  }
  try {
    const signed = await window.nostr.signEvent(unsigned);
    deliver(JSON.stringify(signed), null);
  } catch (e) {
    deliver(null, `window.nostr.signEvent rejected: ${String(e)}`);
  }
}
