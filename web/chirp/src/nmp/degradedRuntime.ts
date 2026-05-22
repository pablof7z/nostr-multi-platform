import type { RuntimeStatus, WorkerEvent, WorkerRequest } from "./protocol";

export type DegradedRuntimeMode = "browser_actor_driver_missing" | "browser_bridge_unavailable";

export class DegradedRuntime {
  private status: RuntimeStatus = "ready";

  constructor(
    private readonly mode: DegradedRuntimeMode = "browser_actor_driver_missing",
    private readonly unavailableReason = "nmp-wasm actor driver is not linked into the web worker yet",
  ) {}

  handle(request: WorkerRequest): WorkerEvent[] {
    switch (request.type) {
      case "hello":
        return this.hello(request.protocol_version);
      case "start":
        this.status = { degraded: this.mode };
        return [
          {
            type: "runtime_status",
            status: this.status,
            correlation_id: request.correlation_id,
          },
        ];
      case "dispatch":
        return [
          {
            type: "capability_failure",
            capability: request.action_type,
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "chirp_action":
        return [
          {
            type: "capability_failure",
            capability: request.type,
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "capability_result":
        return [
          {
            type: "capability_failure",
            capability: request.capability,
            correlation_id: request.correlation_id,
            reason: "capability completions require a running actor",
          },
        ];
      case "stop":
        this.status = "stopped";
        return [
          {
            type: "runtime_status",
            status: this.status,
            correlation_id: request.correlation_id,
          },
        ];
    }
  }

  private hello(version: number): WorkerEvent[] {
    if (version !== 1) {
      return [
        {
          type: "error",
          code: "protocol_mismatch",
          message: `expected protocol 1, got ${version}`,
        },
      ];
    }
    return [{ type: "hello_accepted", protocol_version: 1, status: this.status }];
  }
}
