import type { WorkerRequest } from "@nmp/runtime-web";

/** #1753 S6 — the main-thread NIP-07 broker.
 *
 * Web Workers have no `window.nostr`, so the wasm worker emits a `sign_request`
 * and the MAIN THREAD fulfils it here: call `window.nostr.signEvent`, then post
 * the result back as a `deliver_signer_response` so the worker resumes its
 * parked sign op (pure message re-entry — no polling, D8).
 *
 * `post` is the worker's message sink (`worker.postMessage`). Every failure
 * mode (no extension, malformed event, user rejection) is posted back with
 * `error` set so the worker fails the round-trip closed (D6) rather than
 * leaving the op parked forever. The signature is not published here — the
 * worker routes successful publish continuations through the Rust kernel's
 * `publish_pre_signed` path.
 */
export async function fulfilSignRequestViaExtension(
  post: (request: WorkerRequest) => void,
  correlationId: string,
  unsignedJson: string,
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
