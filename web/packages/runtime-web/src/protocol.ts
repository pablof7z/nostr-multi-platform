// Shared NMP web worker protocol. Every NMP web app uses these types.
// App-specific request/event variants (e.g. chirp's app_action) are
// defined locally in each app and extend WorkerRequest/WorkerEvent there.

export type WorkerRequest =
  | { type: "hello"; app_id: string; platform: string; protocol_version: number }
  | {
      type: "start";
      app_id: string;
      // Optional: apps that bootstrap from a default relay set omit these.
      relays?: string[];
      relay_bootstrap?: { url: string; role: string }[];
      database_name: string;
      correlation_id: string;
    }
  | {
      type: "dispatch";
      action_type: string;
      payload: unknown;
      correlation_id: string;
    }
  | {
      type: "capability_result";
      capability: string;
      correlation_id: string;
      payload: unknown;
    }
  | { type: "stop"; correlation_id: string }
  | {
      type: "set_signer";
      kind: string;
      pubkey_hex: string;
      correlation_id: string;
    };

export type RuntimeStatus =
  | "ready"
  | "running"
  | "stopped"
  | {
      degraded:
        | "browser_actor_driver_missing"
        | "browser_bridge_unavailable"
        | "capability_rejected"
        | "protocol_mismatch";
    };

export type WorkerEvent =
  | { type: "hello_accepted"; protocol_version: number; status: RuntimeStatus }
  | { type: "runtime_status"; status: RuntimeStatus; correlation_id?: string }
  | { type: "action_accepted"; action_type: string; correlation_id: string }
  | { type: "update_bytes"; bytes: Uint8Array }
  | {
      type: "capability_failure";
      capability: string;
      correlation_id: string;
      reason: string;
    }
  | { type: "error"; code: string; message: string; correlation_id?: string };

export const protocolVersion = 1;

export function eventCorrelationId(event: WorkerEvent): string | undefined {
  switch (event.type) {
    case "runtime_status":
    case "action_accepted":
    case "error":
      return event.correlation_id;
    case "capability_failure":
      return event.correlation_id;
    case "hello_accepted":
    case "update_bytes":
      return undefined;
  }
}

export function labelRuntimeStatus(status: RuntimeStatus): string {
  if (typeof status === "string") {
    return status.replace("_", " ");
  }
  return status.degraded.split("_").join(" ");
}
