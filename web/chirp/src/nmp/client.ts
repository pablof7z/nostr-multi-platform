import { DegradedRuntime } from "./degradedRuntime";
import {
  eventCorrelationId,
  protocolVersion,
  type RuntimeStatus,
  type WorkerEvent,
  type WorkerRequest,
  type ChirpAction,
} from "./protocol";
import type { RuntimeCommand } from "./actions";
import { decodeUpdateFrameBytes, SNAPSHOT_SCHEMA_VERSION, UpdateFrameDecodeError } from "./updateFrame";

export type RuntimeSnapshot = {
  status: RuntimeStatus;
  events: WorkerEvent[];
  latestUpdate?: unknown;
  latestUpdateBytes?: Uint8Array;
};

export type RuntimeConnection = {
  appId: string;
  databaseName: string;
};

export const runtimeConnection: RuntimeConnection = {
  appId: "chirp",
  databaseName: "chirp-web",
};

export type NmpClient = {
  snapshot(): RuntimeSnapshot;
  subscribe(listener: (snapshot: RuntimeSnapshot) => void): () => void;
  start(): Promise<RuntimeSnapshot>;
  dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot>;
  dispatchCommand(command: RuntimeCommand): Promise<RuntimeSnapshot>;
  dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot>;
};

export function createNmpClient(): NmpClient {
  if (typeof Worker === "undefined") {
    return new InProcessNmpClient();
  }
  try {
    return new WorkerNmpClient();
  } catch (workerError) {
    console.warn(
      "[nmp] Worker construction failed — falling back to in-process runtime (degraded performance):",
      workerError,
    );
    return new InProcessNmpClient();
  }
}

abstract class BaseClient implements NmpClient {
  private events: WorkerEvent[] = [];
  private latestUpdate: unknown;
  private latestUpdateBytes: Uint8Array | undefined;
  private status: RuntimeStatus = "ready";
  private listeners = new Set<(snapshot: RuntimeSnapshot) => void>();

  snapshot(): RuntimeSnapshot {
    return {
      status: this.status,
      events: [...this.events],
      latestUpdate: this.latestUpdate,
      latestUpdateBytes: this.latestUpdateBytes,
    };
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
    if (event.type === "update_bytes") {
      const bytes = event.bytes instanceof Uint8Array ? event.bytes : new Uint8Array(event.bytes);
      this.latestUpdateBytes = bytes;
      try {
        const decoded = decodeUpdateFrameBytes(bytes);
        if (decoded.type === "snapshot") {
          // Mirror iOS (KernelBridge.swift:525-528): a mismatch on either the
          // frame envelope or the inner payload `schema_version` means the
          // kernel's wire layout moved under us; binding renamed/retyped
          // fields would silently misrender. Keep the last good snapshot so
          // the UI degrades without flashing empty.
          const payloadVersion =
            decoded.payload &&
            typeof decoded.payload === "object" &&
            !Array.isArray(decoded.payload)
              ? (decoded.payload as Record<string, unknown>).schema_version
              : undefined;
          if (
            decoded.schemaVersion !== SNAPSHOT_SCHEMA_VERSION ||
            payloadVersion !== SNAPSHOT_SCHEMA_VERSION
          ) {
            this.status = { degraded: "protocol_mismatch" };
          } else {
            this.latestUpdate = { t: "snapshot", v: decoded.payload };
          }
        } else {
          this.status = { degraded: "browser_actor_driver_missing" };
        }
      } catch (error) {
        if (error instanceof UpdateFrameDecodeError) {
          this.status = { degraded: "browser_actor_driver_missing" };
        } else {
          throw error;
        }
      }
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
  dispatchCommand(command: RuntimeCommand): Promise<RuntimeSnapshot> {
    return this.dispatch(command.actionType, command.payload);
  }
  abstract dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot>;
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
      app_id: runtimeConnection.appId,
      platform: "web",
      protocol_version: protocolVersion,
    } satisfies WorkerRequest);
  }

  async start(): Promise<RuntimeSnapshot> {
    await this.helloReady;
    return this.request({
      type: "start",
      app_id: runtimeConnection.appId,
      database_name: runtimeConnection.databaseName,
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

  async dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot> {
    await this.helloReady;
    return this.request({
      type: "chirp_action",
      action,
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
  private readonly runtime = new DegradedRuntime(
    "browser_bridge_unavailable",
    "Web Worker support is unavailable, so the nmp-wasm bridge cannot start",
  );

  constructor() {
    super();
    this.send({
      type: "hello",
      app_id: runtimeConnection.appId,
      platform: "web",
      protocol_version: protocolVersion,
    });
  }

  async start(): Promise<RuntimeSnapshot> {
    return this.send({
      type: "start",
      app_id: runtimeConnection.appId,
      database_name: runtimeConnection.databaseName,
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

  async dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot> {
    return this.send({
      type: "chirp_action",
      action,
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
