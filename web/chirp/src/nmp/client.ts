import * as flatbuffers from "flatbuffers";

import { DegradedRuntime } from "@nmp/runtime-web";
import { type FeedItem } from "./feedProjection";
import { RefEventStore, type ClaimedEventWire } from "./refEventStore";
import type { EmbeddedEventModel } from "@nmp/components-web/src/content-kind-registry/NostrKindRegistry";
import { RefProfileStore } from "./refProfileStore";
import type { ProfileWire } from "../components/user-avatar/ProfileWire";
import { FrameKind, UpdateFrame } from "./generated/nmp/transport";
import {
  eventCorrelationId,
  protocolVersion,
  type RuntimeStatus,
  type WorkerEvent,
  type WorkerRequest,
  type ChirpAction,
} from "@nmp/runtime-web";
import { chirpActionRequest, type RuntimeCommand } from "./actions";
import { chirpStartRelays, type ChirpRelayStartOverride } from "../chirpConfig";
import { fulfilSignRequestViaExtension } from "./signBroker";
import {
  decodeUpdateFrameBytes,
  SNAPSHOT_SCHEMA_VERSION,
  UpdateFrameDecodeError,
  type DecodedRelayStatus,
} from "./updateFrame";
import { makeCorrelationId } from "./correlationId";
import { applySnapshotSidecars, type SidecarState } from "./clientSnapshotSidecars";

export { makeCorrelationId } from "./correlationId";

export type RuntimeSnapshot = {
  status: RuntimeStatus;
  /** Active runtime path: real worker or in-process degraded fallback. */
  clientRuntime: "worker" | "in_process_fallback";
  events: WorkerEvent[];
  latestUpdateBytes?: Uint8Array;
  /** Snapshot revision decoded cheaply for the Inspector pulse strip. */
  latestRev?: bigint;
  /** Per-relay status rows from the Tier-3 relay_statuses field. Carries raw
   *  protocol tokens; inspector panels derive their own hue (#1768). */
  latestRelayStatuses?: DecodedRelayStatus[];
  /** Decoded nmp.feed.home items. Keep-last-good: corrupt frames do not clear. */
  feedItems?: FeedItem[];
  /** Materialised refs.profile map: hex pubkey -> ProfileWire. Replaced only
   *  when the row-delta cache changes so SolidJS memos do not churn. */
  profileCards?: Map<string, ProfileWire>;
  /** Kernel-authored routing diagnostics JSON, undefined until requested. */
  latestRoutingDecisionsJson?: string;
  /** Materialised `refs.event` map: primary_id -> resolved KCEV event row.
   *  Event-ref content nodes claim these rows; the raw row is the quote-card
   *  fallback when the typed embed projection has not yet surfaced. */
  eventCards?: Map<string, ClaimedEventWire>;
  /** #1767 — kernel-RESOLVED embed envelopes keyed by primary_id, decoded from
   *  the `claimed_event_embeds_json` sidecar. The `projection` is already
   *  kind-dispatched in Rust; event-ref content nodes render the typed card
   *  (article / highlight / quote) from it without re-parsing tags. */
  eventEmbeds?: Map<string, EmbeddedEventModel>;
};

export type RuntimeConnection = {
  appId: string;
  databaseName: string;
};

type FrameSnapshotState = Pick<
  RuntimeSnapshot,
  | "latestUpdateBytes"
  | "latestRev"
  | "latestRelayStatuses"
  | "feedItems"
  | "profileCards"
  | "eventCards"
  | "eventEmbeds"
>;

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
  setSigner(pubkeyHex: string): Promise<RuntimeSnapshot>;
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
  private currentFrame: Partial<FrameSnapshotState> = {};
  private readonly refProfiles = new RefProfileStore();
  private readonly refEvents = new RefEventStore();
  private latestRoutingDecisionsJson: string | undefined;
  private status: RuntimeStatus = "ready";
  private listeners = new Set<(snapshot: RuntimeSnapshot) => void>();

  constructor(private readonly clientRuntime: RuntimeSnapshot["clientRuntime"]) {}

  snapshot(): RuntimeSnapshot {
    return {
      status: this.status,
      clientRuntime: this.clientRuntime,
      events: [...this.events],
      ...this.currentFrame,
      latestRoutingDecisionsJson: this.latestRoutingDecisionsJson,
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
      this.currentFrame = { latestUpdateBytes: bytes };
      try {
        // Hot path: lite=true keeps inspector-only arrays lazy. The worker
        // boundary already applies projection Changed/Cleared retention, so
        // this client decodes the current frame instead of owning merge policy.
        const decoded = decodeUpdateFrameBytes(bytes, { lite: true });
        if (decoded.type === "snapshot") {
          if (decoded.schemaVersion !== SNAPSHOT_SCHEMA_VERSION) {
            this.status = { degraded: "protocol_mismatch" };
          } else {
            // running() mirrors the kernel's run-state; prefer the explicit
            // Tier-3 field over waiting for a separate runtime_status event
            // so the UI reflects live kernel state on every frame.
            if (decoded.running) {
              this.status = "running";
            }
            // Second pass over the same bytes to access feed/ref typed projections.
            try {
              const bb = new flatbuffers.ByteBuffer(bytes);
              if (UpdateFrame.bufferHasIdentifier(bb)) {
                const frame = UpdateFrame.getRootAsUpdateFrame(bb);
                if (frame.kind() === FrameKind.Snapshot) {
                  const snap = frame.snapshot();
                  if (snap) {
                    const sidecarState: SidecarState = {
                      latestFeedItems: undefined as FeedItem[] | undefined,
                      latestProfileCards: undefined,
                      latestEventCards: undefined,
                      latestEventEmbeds: undefined as Map<string, EmbeddedEventModel> | undefined,
                    };
                    applySnapshotSidecars(snap, this.refProfiles, this.refEvents, sidecarState);
                    this.currentFrame = {
                      latestUpdateBytes: bytes,
                      latestRelayStatuses: decoded.relayStatuses,
                      latestRev: decoded.rev,
                      feedItems: sidecarState.latestFeedItems,
                      profileCards: sidecarState.latestProfileCards,
                      eventCards: sidecarState.latestEventCards,
                      eventEmbeds: sidecarState.latestEventEmbeds,
                    };
                    // #1768 — relay-status hue is derived shell-side from the
                    // raw `status` / `auth` / `role` tokens in the inspector
                    // panels (see `relayDiagnosticsTone`); no KRDG tone decode.
                  }
                }
              }
            } catch {
              // Bytes-only currentFrame was installed before decode.
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
    if (event.type === "routing_decisions") {
      this.latestRoutingDecisionsJson = event.json;
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
  abstract setSigner(pubkeyHex: string): Promise<RuntimeSnapshot>;
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

  async setSigner(pubkeyHex: string): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlation_id = makeCorrelationId("web-signer", this.nextCorrelationId++);
    return this.request({ type: "set_identity", kind: "nip07", pubkey_hex: pubkeyHex, correlation_id });
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
    // and posts deliver_signer_response back (pure message re-entry, no polling).
    if (event.type === "sign_request") {
      void fulfilSignRequestViaExtension(
        (request) => this.worker.postMessage(request),
        event.correlation_id,
        event.unsigned_json,
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
    // Degraded runtime ignores relays — relay connectivity is not available —
    // but the wire type still requires explicit host relay policy (#1125).
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

  async setSigner(pubkeyHex: string): Promise<RuntimeSnapshot> {
    return this.send({
      type: "set_identity",
      kind: "nip07",
      pubkey_hex: pubkeyHex,
      correlation_id: makeCorrelationId("web-signer", this.nextCorrelationId++),
    });
  }

  beginSign(accountPubkey: string, unsignedJson: string): void {
    // Degraded runtime has no kernel — begin_sign returns a sign_failed.
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
