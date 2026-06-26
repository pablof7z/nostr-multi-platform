// NmpClient — Item B thin-shell worker bridge.
//
// Owns the worker lifecycle (boot / hello / start), the snapshot subscription
// API, and the NIP-07 sign broker wiring. Snapshot bytes are stored raw for
// Item C to decode (FlatBuffers feed/profile projections are out of scope here).
//
// Mount-point summary for other Items:
//   subscribe(listener)  — Item C uses this (via NmpClientContext) to decode
//                          raw bytes and render feed/profile UI.
//   dispatchCommand()    — Item C calls for ref-resolution bookkeeping;
//                          Item D calls for identity/signing commands.
//   setSigner()          — Item D calls after NIP-07 getPublicKey().
//   sign_request event   — wired to fulfilSignRequestViaExtension(); Item D
//                          UI overlays state from snapshot().events.

import { DegradedRuntime } from "@nmp/runtime-web";
import {
  eventCorrelationId,
  type IdentityRelayPermission,
  protocolVersion,
  type RuntimeStatus,
  type WorkerEvent,
  type WorkerRequest,
} from "@nmp/runtime-web";
import { chirpActionRequest, type ChirpAction, type RuntimeCommand } from "./actions";
import { chirpStartRelays, type ChirpRelayStartOverride } from "../chirpConfig";
import { fulfilSignRequestViaExtension } from "./signBroker";
import { makeCorrelationId } from "./correlationId";

export { makeCorrelationId } from "./correlationId";

/** Snapshot emitted after every worker event.
 *
 * `latestUpdateBytes` carries the raw FlatBuffers `UpdateFrame` bytes from the
 * most recent `update_bytes` event. Item C decodes these into typed projections
 * (feed items, profile cards, relay statuses). Item B only surfaces the raw
 * bytes so the test hook `data-has-snapshot` can assert their presence. */
export type RuntimeSnapshot = {
  status: RuntimeStatus;
  /** Active runtime path: real worker or in-process degraded fallback. */
  clientRuntime: "worker" | "in_process_fallback";
  events: WorkerEvent[];
  /** Raw UpdateFrame bytes from the most recent `update_bytes` event.
   *  Undefined until the first frame arrives. Item C decodes this. */
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
  /** Start the runtime; optional relays override Chirp defaults for tests/dev. */
  start(relays?: ChirpRelayStartOverride): Promise<RuntimeSnapshot>;
  dispatchCommand(command: RuntimeCommand): Promise<RuntimeSnapshot>;
  dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot>;
  /** Install a NIP-07 signer. The host must call window.nostr.getPublicKey()
   *  first and supply the resulting hex pubkey. The wasm runtime installs the
   *  signer synchronously; subsequent write actions use it. */
  setSigner(pubkeyHex: string, identityRelays?: IdentityRelayPermission[]): Promise<RuntimeSnapshot>;
  /** Install a local-key signer from a raw nsec bech32 string. The secret key is
   *  handed to the Rust LocalKey provider VERBATIM — the runtime decodes it,
   *  derives the pubkey, and owns all signing. No crypto runs in TS: the nsec is
   *  never decoded or signed-with on the main thread (Chirp thin-shell rule). */
  setLocalKeySigner(
    secretKeyBech32: string,
    identityRelays?: IdentityRelayPermission[],
  ): Promise<RuntimeSnapshot>;
  /** S6 — parks a NIP-07 sign op and emits sign_request for the main thread. */
  beginSign(accountPubkey: string, unsignedJson: string): void;
  /** #968 — request the kernel-owned routing diagnostics snapshot. */
  refreshRoutingDecisions(): Promise<RuntimeSnapshot>;
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
  private latestUpdateBytes: Uint8Array | undefined;
  private status: RuntimeStatus = "ready";
  private listeners = new Set<(snapshot: RuntimeSnapshot) => void>();

  constructor(private readonly clientRuntime: RuntimeSnapshot["clientRuntime"]) {}

  snapshot(): RuntimeSnapshot {
    return {
      status: this.status,
      clientRuntime: this.clientRuntime,
      events: [...this.events],
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
      // Normalise: worker.ts may post a plain ArrayBuffer that structured-clone
      // transferred; ensure we always hold a Uint8Array view.
      const bytes = event.bytes instanceof Uint8Array ? event.bytes : new Uint8Array(event.bytes);
      this.latestUpdateBytes = bytes;
      // Mirror the kernel's run-state on first frame (same logic as before
      // but without the schema-version decode which needs FlatBuffers types).
      this.status = "running";
    }
    this.events = [event, ...this.events].slice(0, 32);
    const snapshot = this.snapshot();
    for (const listener of this.listeners) {
      listener(snapshot);
    }
    return snapshot;
  }

  abstract start(relays?: ChirpRelayStartOverride): Promise<RuntimeSnapshot>;
  abstract dispatchCommand(command: RuntimeCommand): Promise<RuntimeSnapshot>;
  abstract dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot>;
  abstract setSigner(pubkeyHex: string, identityRelays?: IdentityRelayPermission[]): Promise<RuntimeSnapshot>;
  abstract setLocalKeySigner(
    secretKeyBech32: string,
    identityRelays?: IdentityRelayPermission[],
  ): Promise<RuntimeSnapshot>;
  abstract beginSign(accountPubkey: string, unsignedJson: string): void;
  abstract refreshRoutingDecisions(): Promise<RuntimeSnapshot>;
}

class WorkerNmpClient extends BaseClient {
  private readonly worker = new Worker(new URL("@nmp/runtime-web/worker", import.meta.url), {
    type: "module",
  });
  private readonly pending = new Map<string, (snapshot: RuntimeSnapshot) => void>();
  private helloReady: Promise<void>;
  private resolveHello?: () => void;
  private nextCorrelationId = 0;

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

  async start(relays?: ChirpRelayStartOverride): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const { relays: startRelays, relay_bootstrap } = chirpStartRelays(relays);
    return this.request({
      type: "start",
      app_id: runtimeConnection.appId,
      relays: startRelays,
      relay_bootstrap,
      database_name: runtimeConnection.databaseName,
      correlation_id: "web-start",
    });
  }

  override async dispatchCommand(command: RuntimeCommand): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web", this.nextCorrelationId++);
    switch (command.kind) {
      case "dispatch_bytes": {
        const bytes = command.buildDispatchBytes(correlationId);
        return this.request({ type: "dispatch_bytes", bytes }, correlationId);
      }
      case "resolve_ref":
        return this.request(
          {
            type: "resolve_ref",
            namespace: command.namespace,
            key: command.key,
            consumer_id: command.consumerId,
            shape: command.shape,
            liveness: command.liveness,
            hints: command.hints ?? [],
            event_author: command.eventAuthor ?? null,
            correlation_id: correlationId,
          },
          correlationId,
        );
      case "release_ref":
        return this.request(
          {
            type: "release_ref",
            namespace: command.namespace,
            key: command.key,
            consumer_id: command.consumerId,
            correlation_id: correlationId,
          },
          correlationId,
        );
      case "unsupported":
        return this.record({
          type: "capability_failure",
          capability: command.capability,
          correlation_id: correlationId,
          reason: command.reason,
        });
    }
  }

  async dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web", this.nextCorrelationId++);
    return this.request(chirpActionRequest(action, correlationId), correlationId);
  }

  async setSigner(pubkeyHex: string, identityRelays?: IdentityRelayPermission[]): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlation_id = makeCorrelationId("web-signer", this.nextCorrelationId++);
    return this.request({
      type: "set_identity",
      kind: "nip07",
      pubkey_hex: pubkeyHex,
      correlation_id,
      identity_relays: identityRelays,
    });
  }

  async setLocalKeySigner(
    secretKeyBech32: string,
    identityRelays?: IdentityRelayPermission[],
  ): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlation_id = makeCorrelationId("web-signer", this.nextCorrelationId++);
    return this.request({
      type: "set_identity",
      kind: "local_key",
      // Empty pubkey — the runtime derives it from the secret key. Present so
      // the request deserializes against the runtime's required field.
      pubkey_hex: "",
      secret_key_bech32: secretKeyBech32,
      correlation_id,
      identity_relays: identityRelays,
    });
  }

  beginSign(accountPubkey: string, unsignedJson: string): void {
    this.worker.postMessage({
      type: "begin_sign",
      account_pubkey: accountPubkey,
      unsigned_json: unsignedJson,
    } satisfies WorkerRequest);
  }

  async refreshRoutingDecisions(): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-routing", this.nextCorrelationId++);
    return this.request({ type: "routing_decisions", correlation_id: correlationId }, correlationId);
  }

  private request(request: WorkerRequest, explicitCorrelationId?: string): Promise<RuntimeSnapshot> {
    const correlationId =
      explicitCorrelationId ?? ("correlation_id" in request ? request.correlation_id : undefined);
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
    // S6 — broker: worker emits sign_request; main thread calls window.nostr.signEvent
    // and posts deliver_signer_response back (pure message re-entry, no polling D8).
    if (event.type === "sign_request") {
      void fulfilSignRequestViaExtension(
        (request) => this.worker.postMessage(request),
        event.correlation_id,
        event.unsigned_json,
        event.account_pubkey,
      );
      return;
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
  private nextCorrelationId = 0;

  constructor() {
    super("in_process_fallback");
    this.send({
      type: "hello",
      app_id: runtimeConnection.appId,
      platform: "web",
      protocol_version: protocolVersion,
    });
  }

  async start(relays?: ChirpRelayStartOverride): Promise<RuntimeSnapshot> {
    const { relays: startRelays, relay_bootstrap } = chirpStartRelays(relays);
    return this.send({
      type: "start",
      app_id: runtimeConnection.appId,
      relays: startRelays,
      relay_bootstrap,
      database_name: runtimeConnection.databaseName,
      correlation_id: "web-start",
    });
  }

  override async dispatchCommand(command: RuntimeCommand): Promise<RuntimeSnapshot> {
    const correlationId = makeCorrelationId("web", this.nextCorrelationId++);
    switch (command.kind) {
      case "dispatch_bytes": {
        const bytes = command.buildDispatchBytes(correlationId);
        return this.send({ type: "dispatch_bytes", bytes });
      }
      case "resolve_ref":
        return this.send({
          type: "resolve_ref",
          namespace: command.namespace,
          key: command.key,
          consumer_id: command.consumerId,
          shape: command.shape,
          liveness: command.liveness,
          hints: command.hints ?? [],
          event_author: command.eventAuthor ?? null,
          correlation_id: correlationId,
        });
      case "release_ref":
        return this.send({
          type: "release_ref",
          namespace: command.namespace,
          key: command.key,
          consumer_id: command.consumerId,
          correlation_id: correlationId,
        });
      case "unsupported":
        return this.record({
          type: "capability_failure",
          capability: command.capability,
          correlation_id: correlationId,
          reason: command.reason,
        });
    }
  }

  async dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot> {
    const correlationId = makeCorrelationId("web", this.nextCorrelationId++);
    return this.send(chirpActionRequest(action, correlationId));
  }

  async setSigner(pubkeyHex: string, identityRelays?: IdentityRelayPermission[]): Promise<RuntimeSnapshot> {
    return this.send({
      type: "set_identity",
      kind: "nip07",
      pubkey_hex: pubkeyHex,
      correlation_id: makeCorrelationId("web-signer", this.nextCorrelationId++),
      identity_relays: identityRelays,
    });
  }

  async setLocalKeySigner(
    secretKeyBech32: string,
    identityRelays?: IdentityRelayPermission[],
  ): Promise<RuntimeSnapshot> {
    return this.send({
      type: "set_identity",
      kind: "local_key",
      // Empty pubkey — the runtime derives it from the secret key.
      pubkey_hex: "",
      secret_key_bech32: secretKeyBech32,
      correlation_id: makeCorrelationId("web-signer", this.nextCorrelationId++),
      identity_relays: identityRelays,
    });
  }

  beginSign(accountPubkey: string, unsignedJson: string): void {
    this.send({ type: "begin_sign", account_pubkey: accountPubkey, unsigned_json: unsignedJson });
  }

  async refreshRoutingDecisions(): Promise<RuntimeSnapshot> {
    return this.send({
      type: "routing_decisions",
      correlation_id: makeCorrelationId("web-routing", this.nextCorrelationId++),
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
