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
import { decodeUpdateFrameBytes, SNAPSHOT_SCHEMA_VERSION, UpdateFrameDecodeError, type DecodedRelayStatus } from "./updateFrame";

export type RuntimeSnapshot = {
  status: RuntimeStatus;
  /** Identifies which runtime path is active.
   *  "worker"              — Web Worker launched; nmp-wasm will load inside it.
   *  "in_process_fallback" — Worker construction failed or Worker API is absent;
   *                          the client wraps a DegradedRuntime that returns
   *                          capability_failure for every action. */
  clientRuntime: "worker" | "in_process_fallback";
  events: WorkerEvent[];
  latestUpdate?: unknown;
  latestUpdateBytes?: Uint8Array;
  /** Per-relay status rows decoded from the Tier-3 relay_statuses field of the
   *  most recent SnapshotFrame. Populated after the first successful decode;
   *  undefined before any snapshot arrives. Empty array means the kernel
   *  has no relays configured yet. */
  latestRelayStatuses?: DecodedRelayStatus[];
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
  /** Start the runtime. Pass `relays` to override the built-in chirp relay
   *  list (used by the Playwright smoke test to inject the fixture relay). */
  start(relays?: string[]): Promise<RuntimeSnapshot>;
  dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot>;
  dispatchCommand(command: RuntimeCommand): Promise<RuntimeSnapshot>;
  dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot>;
};

export function createNmpClient(): NmpClient {
  if (typeof Worker === "undefined") {
    console.warn(
      "[nmp] Web Worker API is unavailable (SSR, CSP, or browser restriction). " +
        "Falling back to in-process degraded runtime — every action will return capability_failure.",
    );
    return new InProcessNmpClient();
  }
  try {
    return new WorkerNmpClient();
  } catch (err) {
    console.warn(
      "[nmp] Worker construction failed — falling back to in-process degraded runtime. " +
        "Every action will return capability_failure. Worker error:",
      err,
    );
    return new InProcessNmpClient();
  }
}

abstract class BaseClient implements NmpClient {
  private events: WorkerEvent[] = [];
  private latestUpdate: unknown;
  private latestUpdateBytes: Uint8Array | undefined;
  private latestRelayStatuses: DecodedRelayStatus[] | undefined;
  private status: RuntimeStatus = "ready";
  private listeners = new Set<(snapshot: RuntimeSnapshot) => void>();

  constructor(private readonly clientRuntime: RuntimeSnapshot["clientRuntime"]) {}

  snapshot(): RuntimeSnapshot {
    return {
      status: this.status,
      clientRuntime: this.clientRuntime,
      events: [...this.events],
      latestUpdate: this.latestUpdate,
      latestUpdateBytes: this.latestUpdateBytes,
      latestRelayStatuses: this.latestRelayStatuses,
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
          // Envelope schema version mismatch: the kernel's wire layout moved
          // under us. Mirror iOS (KernelBridge.swift:525-528): keep the last
          // good snapshot so the UI degrades without flashing empty.
          if (decoded.schemaVersion !== SNAPSHOT_SCHEMA_VERSION) {
            this.status = { degraded: "protocol_mismatch" };
          } else {
            // Tier-3 relay statuses: surfaced directly from the FlatBuffers
            // envelope without going through the typed-projection sidecar.
            this.latestRelayStatuses = decoded.relayStatuses;
            // running() mirrors the kernel's run-state; prefer the explicit
            // Tier-3 field over waiting for a separate runtime_status event
            // so the UI reflects live kernel state on every frame.
            if (decoded.running) {
              this.status = "running";
            }
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

  abstract start(relays?: string[]): Promise<RuntimeSnapshot>;
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
    super("worker");
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

  async start(relays?: string[]): Promise<RuntimeSnapshot> {
    await this.helloReady;
    // When relay URLs are supplied (e.g. from the Playwright smoke test via
    // the ?relay= query parameter), pass them as relay_bootstrap so the wasm
    // runtime uses them instead of its built-in chirp defaults. The wasm
    // StartConfig serde-defaults relay_bootstrap to the chirp list, so the
    // explicit non-empty array takes precedence (relay_bootstrap_from_config).
    const relayBootstrap =
      relays && relays.length > 0
        ? relays.map((url) => ({ url, role: "both" as const }))
        : undefined;
    return this.request({
      type: "start",
      app_id: runtimeConnection.appId,
      database_name: runtimeConnection.databaseName,
      correlation_id: "web-start",
      ...(relayBootstrap ? { relay_bootstrap: relayBootstrap } : {}),
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
    super("in_process_fallback");
    this.send({
      type: "hello",
      app_id: runtimeConnection.appId,
      platform: "web",
      protocol_version: protocolVersion,
    });
  }

  async start(_relays?: string[]): Promise<RuntimeSnapshot> {
    // Degraded runtime ignores relays — relay connectivity is not available.
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
