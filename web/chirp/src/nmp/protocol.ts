// Chirp worker protocol — extends the shared @nmp/runtime-web base protocol
// with the chirp-specific app_action request and ChirpAction type.
export type {
  WorkerEvent,
  RuntimeStatus,
  protocolVersion,
  eventCorrelationId,
  labelRuntimeStatus,
} from "@nmp/runtime-web";
export { protocolVersion, eventCorrelationId, labelRuntimeStatus } from "@nmp/runtime-web";
import type { WorkerRequest as BaseWorkerRequest } from "@nmp/runtime-web";

export type ChirpAction =
  | { action: "publish_note"; content: string; reply_to_id?: string | null }
  | { action: "react"; target_event_id: string; reaction?: string }
  | { action: "follow"; pubkey: string }
  | { action: "unfollow"; pubkey: string };

// Extends the base WorkerRequest union with chirp-specific app_action.
// The WASM kernel handles app_action via handle_json; the shared DegradedRuntime
// falls through to a generic capability_failure for unknown request types.
export type WorkerRequest =
  | BaseWorkerRequest
  | {
      type: "app_action";
      action: ChirpAction;
      correlation_id: string;
    };
