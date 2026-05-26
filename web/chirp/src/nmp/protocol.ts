export type WorkerRequest =
  | { type: "hello"; app_id: string; platform: string; protocol_version: number }
  | {
      type: "start";
      app_id: string;
      relays?: string[];
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
      type: "chirp_action";
      action: ChirpAction;
      correlation_id: string;
    }
  | {
      type: "capability_result";
      capability: string;
      correlation_id: string;
      payload: unknown;
    }
  | { type: "stop"; correlation_id: string };

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

export type ChirpAction =
  | { action: "publish_note"; content: string; reply_to_id?: string | null }
  | { action: "react"; target_event_id: string; reaction?: string }
  | { action: "follow"; pubkey: string }
  | { action: "unfollow"; pubkey: string };

export const protocolVersion = 1;

export function eventCorrelationId(event: WorkerEvent): string | undefined {
  switch (event.type) {
    case "runtime_status":
    case "action_accepted":
    case "capability_failure":
    case "error":
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
