import { DegradedRuntime } from "./degradedRuntime";
import {
  eventCorrelationId,
  protocolVersion,
  type RuntimeStatus,
  type WorkerEvent,
  type WorkerRequest,
} from "./protocol";

export type RuntimeSnapshot = {
  status: RuntimeStatus;
  events: WorkerEvent[];
};

export type NmpClient = {
  snapshot(): RuntimeSnapshot;
  subscribe(listener: (snapshot: RuntimeSnapshot) => void): () => void;
  start(): Promise<RuntimeSnapshot>;
  dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot>;
};

export function createNmpClient(): NmpClient {
  if (typeof Worker === "undefined") {
    return new InProcessNmpClient();
  }
  try {
    return new WorkerNmpClient();
  } catch {
    return new InProcessNmpClient();
  }
}

abstract class BaseClient implements NmpClient {
  private events: WorkerEvent[] = [];
  private status: RuntimeStatus = "ready";
  private listeners = new Set<(snapshot: RuntimeSnapshot) => void>();

  snapshot(): RuntimeSnapshot {
    return { status: this.status, events: [...this.events] };
  }

  subscribe(listener: (snapshot: RuntimeSnapshot) => void): () => void {
    this.listeners.add(listener);
    listener(this.snapshot());
    return () => this.listeners.delete(listener);
  }

  protected record(event: WorkerEvent): RuntimeSnapshot {
    if (event.type === "runtime_status" || event.type === "hello_accepted") {
      this.status = event.status;
    }
    this.events = [event, ...this.events].slice(0, 8);
    const snapshot = this.snapshot();
    for (const listener of this.listeners) {
      listener(snapshot);
    }
    return snapshot;
  }

  abstract start(): Promise<RuntimeSnapshot>;
  abstract dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot>;
}

class WorkerNmpClient extends BaseClient {
  private readonly worker = new Worker(new URL("./worker.ts", import.meta.url), {
    type: "module",
  });
  private readonly pending = new Map<string, (snapshot: RuntimeSnapshot) => void>();
  private helloReady: Promise<void>;
  private resolveHello?: () => void;

  constructor() {
    super();
    this.helloReady = new Promise((resolve) => {
      this.resolveHello = resolve;
    });
    this.worker.onmessage = (message: MessageEvent<WorkerEvent>) => {
      this.accept(message.data);
    };
    this.worker.postMessage({
      type: "hello",
      app_id: "chirp",
      platform: "web",
      protocol_version: protocolVersion,
    } satisfies WorkerRequest);
  }

  async start(): Promise<RuntimeSnapshot> {
    await this.helloReady;
    return this.request({
      type: "start",
      app_id: "chirp",
      relays: ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.primal.net"],
      database_name: "chirp-web",
      correlation_id: "web-start",
    });
  }

  async dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot> {
    await this.helloReady;
    return this.request({
      type: "dispatch",
      action_type: actionType,
      payload,
      correlation_id: `web-${Date.now()}`,
    });
  }

  private request(request: WorkerRequest): Promise<RuntimeSnapshot> {
    const correlationId = "correlation_id" in request ? request.correlation_id : undefined;
    if (!correlationId) {
      this.worker.postMessage(request);
      return Promise.resolve(this.snapshot());
    }
    return new Promise((resolve) => {
      this.pending.set(correlationId, resolve);
      this.worker.postMessage(request);
    });
  }

  private accept(event: WorkerEvent) {
    const snapshot = this.record(event);
    if (event.type === "hello_accepted") {
      this.resolveHello?.();
    }
    const correlationId = eventCorrelationId(event);
    if (!correlationId) {
      return;
    }
    const resolve = this.pending.get(correlationId);
    if (resolve) {
      this.pending.delete(correlationId);
      resolve(snapshot);
    }
  }
}

class InProcessNmpClient extends BaseClient {
  private readonly runtime = new DegradedRuntime();

  constructor() {
    super();
    this.send({
      type: "hello",
      app_id: "chirp",
      platform: "web",
      protocol_version: protocolVersion,
    });
  }

  async start(): Promise<RuntimeSnapshot> {
    return this.send({
      type: "start",
      app_id: "chirp",
      relays: ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.primal.net"],
      database_name: "chirp-web",
      correlation_id: "web-start",
    });
  }

  async dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot> {
    return this.send({
      type: "dispatch",
      action_type: actionType,
      payload,
      correlation_id: `web-${Date.now()}`,
    });
  }

  private send(request: WorkerRequest): RuntimeSnapshot {
    let snapshot = this.snapshot();
    for (const event of this.runtime.handle(request)) {
      snapshot = this.record(event);
    }
    return snapshot;
  }
}
