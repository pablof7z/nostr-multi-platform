import type { RuntimeStatus, WorkerEvent, WorkerRequest } from "./protocol";

export class DegradedRuntime {
  private status: RuntimeStatus = "ready";

  handle(request: WorkerRequest): WorkerEvent[] {
    switch (request.type) {
      case "hello":
        return this.hello(request.protocol_version);
      case "start":
        this.status = { degraded: "browser_actor_driver_missing" };
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
            reason: "nmp-wasm actor driver is not linked into the web worker yet",
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
