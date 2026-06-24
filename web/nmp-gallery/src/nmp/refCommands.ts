import type { WorkerRequest } from "@nmp/runtime-web";

const refWire = {
  profile: { namespace: 0, shape: { ref: 0 } },
  event: { namespace: 1, shape: { embed: 0 } },
  liveness: { cacheOk: 0 },
} as const;

export function resolveProfileRefRequest(
  pubkey: string,
  consumerId: string,
  correlationId: string,
): WorkerRequest {
  return {
    type: "dispatch",
    action_type: "nmp.kernel.resolve_ref",
    payload: {
      namespace: refWire.profile.namespace,
      key: pubkey,
      consumer_id: consumerId,
      shape: refWire.profile.shape.ref,
      liveness: refWire.liveness.cacheOk,
    },
    correlation_id: correlationId,
  };
}

export function releaseProfileRefRequest(
  pubkey: string,
  consumerId: string,
  correlationId: string,
): WorkerRequest {
  return {
    type: "dispatch",
    action_type: "nmp.kernel.release_ref",
    payload: {
      namespace: refWire.profile.namespace,
      key: pubkey,
      consumer_id: consumerId,
    },
    correlation_id: correlationId,
  };
}

export function resolveEventEmbedRequest(
  key: string,
  consumerId: string,
  hints: string[],
  correlationId: string,
): WorkerRequest {
  return {
    type: "dispatch",
    action_type: "nmp.kernel.resolve_ref",
    payload: {
      namespace: refWire.event.namespace,
      key,
      consumer_id: consumerId,
      shape: refWire.event.shape.embed,
      liveness: refWire.liveness.cacheOk,
      hints,
    },
    correlation_id: correlationId,
  };
}

export function releaseEventRefRequest(
  key: string,
  consumerId: string,
  correlationId: string,
): WorkerRequest {
  return {
    type: "dispatch",
    action_type: "nmp.kernel.release_ref",
    payload: {
      namespace: refWire.event.namespace,
      key,
      consumer_id: consumerId,
    },
    correlation_id: correlationId,
  };
}
