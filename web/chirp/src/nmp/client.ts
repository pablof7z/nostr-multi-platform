import type { RuntimeStatus, WorkerEvent, WorkerRequest } from "./protocol";

export type RuntimeSnapshot = {
  status: RuntimeStatus;
  events: WorkerEvent[];
};

export type NmpClient = {
  snapshot(): RuntimeSnapshot;
  start(): Promise<RuntimeSnapshot>;
  dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot>;
};

export function createNmpClient(): NmpClient {
  return new DegradedNmpClient();
}

class DegradedNmpClient implements NmpClient {
  private events: WorkerEvent[] = [];
  private status: RuntimeStatus = "ready";

  snapshot(): RuntimeSnapshot {
    return { status: this.status, events: [...this.events] };
  }

  async start(): Promise<RuntimeSnapshot> {
    return this.record({
      type: "runtime_status",
      status: { degraded: "browser_actor_driver_missing" },
      correlation_id: "web-start",
    });
  }

  async dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot> {
    const request: WorkerRequest = {
      type: "dispatch",
      action_type: actionType,
      payload,
      correlation_id: `web-${Date.now()}`,
    };
    return this.record({
      type: "capability_failure",
      capability: request.action_type,
      correlation_id: request.correlation_id,
      reason: "nmp-wasm actor driver is not linked into the web worker yet",
    });
  }

  private record(event: WorkerEvent): RuntimeSnapshot {
    if (event.type === "runtime_status" || event.type === "hello_accepted") {
      this.status = event.status;
    }
    this.events = [event, ...this.events].slice(0, 8);
    return this.snapshot();
  }
}
