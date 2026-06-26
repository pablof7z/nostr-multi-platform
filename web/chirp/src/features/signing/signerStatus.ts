// Signer-status derivation (#2038 item D) — pure, no JSX, no crypto.
//
// The authoritative signer-state lives in the kernel's `signer_state` typed
// FlatBuffers sidecar (#2074, file id `KSST`) carried inside the `UpdateFrame`
// snapshot. Decoding that frame is Item C's responsibility (it owns the
// FlatBuffers `UpdateFrame`/`SnapshotFrame` bindings); chirp's thin shell
// surfaces only the raw bytes. To stay decoupled from Item C, the signing panel
// derives its live status from the worker EVENT stream — `sign_request`,
// `sign_completed`, `sign_failed`, and `capability_failure` — which the shell
// already records in `snapshot().events` (newest-first). This is honest: every
// signal here is emitted by the Rust worker, not synthesised in TS.
//
// When Item C lands a decoded `signer_state` accessor on the snapshot, the panel
// can read the richer `SignerStateModel` (signer_kind/state/reason) directly;
// until then the event-derived view is the available, accurate source.

import type { WorkerEvent } from "@nmp/runtime-web";

/** Lifecycle of the most recent sign capability round-trip. */
export type SignLifecycle =
  | { phase: "idle" }
  | { phase: "pending"; correlationId: string; accountPubkey: string }
  | { phase: "completed"; correlationId: string }
  | { phase: "failed"; correlationId: string; reason: string };

/** Derive the current sign-round-trip lifecycle from the event log.
 *
 * `events` is newest-first (the shell prepends). The latest sign-related event
 * defines the current phase: a `sign_request` with no newer terminal is still
 * pending; a `sign_completed`/`sign_failed` is terminal. */
export function deriveSignLifecycle(events: readonly WorkerEvent[]): SignLifecycle {
  for (const event of events) {
    switch (event.type) {
      case "sign_completed":
        return { phase: "completed", correlationId: event.correlation_id };
      case "sign_failed":
        return { phase: "failed", correlationId: event.correlation_id, reason: event.reason };
      case "sign_request":
        return {
          phase: "pending",
          correlationId: event.correlation_id,
          accountPubkey: event.account_pubkey,
        };
      default:
        continue;
    }
  }
  return { phase: "idle" };
}

/** The most recent capability failure reason, if the latest sign/identity
 *  attempt failed at the capability layer (e.g. no signer installed, identity
 *  rejected). Returns `undefined` when the newest relevant event is not a
 *  failure. Used to surface honest onboarding/sign errors. */
export function latestCapabilityFailure(events: readonly WorkerEvent[]): string | undefined {
  for (const event of events) {
    if (event.type === "capability_failure") {
      return event.reason;
    }
    // Stop at the first terminal/positive signal so a stale failure from earlier
    // in the session doesn't mask a later success.
    if (
      event.type === "sign_completed" ||
      event.type === "action_accepted" ||
      event.type === "update_bytes"
    ) {
      return undefined;
    }
  }
  return undefined;
}
