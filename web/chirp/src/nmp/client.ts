import * as flatbuffers from "flatbuffers";

import { DegradedRuntime } from "@nmp/runtime-web";
import { decodeHomeFeed, findRefsProfileSidecar, type FeedItem } from "./feedProjection";
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
import { chirpActionRequest, typedWriteRequest, TYPED_WRITE_NAMESPACES, type RuntimeCommand } from "./actions";
import { chirpStartRelays } from "../chirpConfig";
import { fulfilSignRequestViaExtension } from "./signBroker";
import {
  decodeUpdateFrameBytes,
  SNAPSHOT_SCHEMA_VERSION,
  UpdateFrameDecodeError,
  type DecodedRelayStatus,
} from "./updateFrame";

/** Builds a unique worker-request correlation id.
 *
 * `Date.now()` has millisecond resolution, so two calls in the same tick
 * (e.g. two component `onMount` handlers that both call `dispatch`) can
 * produce the SAME string — the second `pending.set()` would silently
 * overwrite the first, leaking the first promise.  The monotonic `seq`
 * counter is appended to guarantee uniqueness regardless of timing.
 *
 * @param prefix Human-readable tag (e.g. `"web"`, `"web-signer"`).
 * @param seq    Per-client monotonic counter; caller is responsible for
 *               incrementing it on every call.
 */
export function makeCorrelationId(prefix: string, seq: number): string {
  return `${prefix}-${Date.now()}-${seq}`;
}

/** Structural equality of two materialised `refs.profile` maps. Decides whether
 *  to swap the `latestProfileCards` reference: a no-op frame keeps the same
 *  reference (no feed churn, #1436); ANY content change — including an
 *  identity/epoch rebaseline that shrinks the set to empty — swaps it so the UI
 *  never shows stale cards. */
function profileCardsEqual(
  a: Map<string, ProfileWire> | undefined,
  b: Map<string, ProfileWire>,
): boolean {
  if (a === undefined || a.size !== b.size) return false;
  for (const [key, wa] of a) {
    const wb = b.get(key);
    if (
      !wb ||
      wa.displayName !== wb.displayName ||
      wa.pictureUrl !== wb.pictureUrl ||
      wa.nip05 !== wb.nip05 ||
      wa.about !== wb.about ||
      wa.lnurl !== wb.lnurl ||
      wa.npubShort !== wb.npubShort
    ) {
      return false;
    }
  }
  return true;
}

export type RuntimeSnapshot = {
  status: RuntimeStatus;
  /** Identifies which runtime path is active.
   *  "worker"              — Web Worker launched; nmp-wasm will load inside it.
   *  "in_process_fallback" — Worker construction failed or Worker API is absent;
   *                          the client wraps a DegradedRuntime that returns
   *                          capability_failure for every action. */
  clientRuntime: "worker" | "in_process_fallback";
  events: WorkerEvent[];
  latestUpdateBytes?: Uint8Array;
  /** Snapshot revision number — decoded cheaply on every frame for the
   *  Inspector's collapsed pulse strip. Undefined before the first snapshot. */
  latestRev?: bigint;
  /** Per-relay status rows decoded from the Tier-3 relay_statuses field of the
   *  most recent SnapshotFrame. Populated after the first successful decode;
   *  undefined before any snapshot arrives. Empty array means the kernel
   *  has no relays configured yet. Carries raw protocol tokens only; the
   *  inspector panels derive their own hue from them (#1768). */
  latestRelayStatuses?: DecodedRelayStatus[];
  /** Decoded home feed items from the nmp.feed.home typed projection.
   *  Populated after the first snapshot that contains the projection; undefined
   *  before any projection arrives. Keep-last-good: only overwritten when a
   *  new successful decode arrives (never cleared on a corrupt frame). */
  feedItems?: FeedItem[];
  /** Materialised `refs.profile` map: hex pubkey -> ProfileWire (display name +
   *  picture + nip05 + …). The per-key `refs.profile` row-delta projection is
   *  merged into a stateful `RefProfileStore` (`RefRowCache`); this is its
   *  current full set. Powers BOTH the feed-row display-name join and the
   *  registry user-* components (NostrAvatar needs the picture URL). Replaced by
   *  a fresh `Map` reference only when the cache actually changed this frame
   *  (stable reference across no-op frames so SolidJS memos do not churn). */
  profileCards?: Map<string, ProfileWire>;
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
  /** Install a NIP-07 signer. The host must call window.nostr.getPublicKey()
   *  first and supply the resulting hex pubkey. The wasm runtime installs the
   *  signer synchronously; subsequent write actions use it. */
  setSigner(pubkeyHex: string): Promise<RuntimeSnapshot>;
  /** S6 — NIP-07 sign round-trip: parks a sign op, emits sign_request for the
   *  main-thread broker; resolves via sign_completed / sign_failed (#1007). */
  beginSign(accountPubkey: string, unsignedJson: string): void;
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
  private latestRev: bigint | undefined;
  private latestRelayStatuses: DecodedRelayStatus[] | undefined;
  private latestFeedItems: FeedItem[] | undefined;
  // Stateful per-key `refs.profile` row-delta cache. Lives for the client's
  // lifetime (NOT rebuilt per frame — row deltas merge into it). Materialised
  // into `latestProfileCards` only when the cache changed this frame.
  private readonly refProfiles = new RefProfileStore();
  private latestProfileCards: Map<string, ProfileWire> | undefined;
  private status: RuntimeStatus = "ready";
  private listeners = new Set<(snapshot: RuntimeSnapshot) => void>();

  constructor(private readonly clientRuntime: RuntimeSnapshot["clientRuntime"]) {}

  snapshot(): RuntimeSnapshot {
    return {
      status: this.status,
      clientRuntime: this.clientRuntime,
      events: [...this.events],
      latestUpdateBytes: this.latestUpdateBytes,
      latestRev: this.latestRev,
      latestRelayStatuses: this.latestRelayStatuses,
      feedItems: this.latestFeedItems,
      profileCards: this.latestProfileCards,
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
        // Hot path: lite=true skips the logicalInterests / wireSubscriptions /
        // metrics loops in decodeUpdateFrameBytes. Those arrays are only needed
        // by the Inspector panels when the dock is open; they are decoded lazily
        // there via decodeInspectorSnapshot(). The feed-critical path (relay
        // statuses, rev, feed items, resolved profiles) is kept lean so
        // profile-resolution frames are processed without delay.
        const decoded = decodeUpdateFrameBytes(bytes, { lite: true });
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
            // Rev exposed for the Inspector's collapsed pulse strip.
            this.latestRev = decoded.rev;
            // running() mirrors the kernel's run-state; prefer the explicit
            // Tier-3 field over waiting for a separate runtime_status event
            // so the UI reflects live kernel state on every frame.
            if (decoded.running) {
              this.status = "running";
            }
            // Second pass over the same bytes to access typed projections
            // (feed items, resolved profiles). These are all feed-critical and
            // must run on every frame.
            // Keep-last-good: only overwrite on a non-undefined result.
            try {
              const bb = new flatbuffers.ByteBuffer(bytes);
              if (UpdateFrame.bufferHasIdentifier(bb)) {
                const frame = UpdateFrame.getRootAsUpdateFrame(bb);
                if (frame.kind() === FrameKind.Snapshot) {
                  const snap = frame.snapshot();
                  if (snap) {
                    const feedResult = decodeHomeFeed(snap);
                    if (feedResult !== undefined) {
                      this.latestFeedItems = feedResult.items;
                    }
                    // Merge the `refs.profile` row-delta sidecar into the stateful
                    // RefProfileStore under THIS frame's identity (session_id,
                    // snapshot_epoch). The store handles baseline/incremental/
                    // identity-rebuild + decode-before-commit + fail-closed
                    // internally (ADR-0063); an absent entry leaves it untouched.
                    // After applying we re-derive the FULL set (like native
                    // desktop/tui) and swap `latestProfileCards` ONLY when the
                    // content differs — so an empty rebaseline drops stale cards
                    // (changedKeys alone would not) without churning the feed (#1436).
                    const refsPayload = findRefsProfileSidecar(snap);
                    if (refsPayload !== undefined) {
                      this.refProfiles.applySidecar(
                        refsPayload,
                        snap.sessionId(),
                        snap.snapshotEpoch(),
                      );
                      const next = this.refProfiles.profiles();
                      if (!profileCardsEqual(this.latestProfileCards, next)) {
                        this.latestProfileCards = next;
                      }
                    }
                    // #1768 — relay-status hue is derived shell-side from the
                    // raw `status` / `auth` / `role` tokens in the inspector
                    // panels (see `relayDiagnosticsTone`); no KRDG tone decode.
                  }
                }
              }
            } catch {
              // Corrupt inner buffer: keep last-good feed items.
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
    this.events = [event, ...this.events].slice(0, 32);
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
  abstract setSigner(pubkeyHex: string): Promise<RuntimeSnapshot>;
  abstract beginSign(accountPubkey: string, unsignedJson: string): void;
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

  async start(relays?: string[]): Promise<RuntimeSnapshot> {
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

  // ADR-0064 / #1743: app-level signed writes (TYPED_WRITE_NAMESPACES) cross the
  // typed `dispatch_bytes` doorway (correlation_id lives INSIDE the envelope) — no
  // parallel app-write path. Everything else rides the generic JSON `dispatch` arm.
  async dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web", this.nextCorrelationId++);
    if (TYPED_WRITE_NAMESPACES.has(actionType)) {
      return this.request(typedWriteRequest(actionType, payload, correlationId), correlationId);
    }
    return this.request(
      { type: "dispatch", action_type: actionType, payload, correlation_id: correlationId },
      correlationId,
    );
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

  async start(relays?: string[]): Promise<RuntimeSnapshot> {
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

  async dispatch(actionType: string, payload: unknown): Promise<RuntimeSnapshot> {
    // Same typed-write routing as WorkerNmpClient.dispatch, sent synchronously.
    const correlationId = makeCorrelationId("web", this.nextCorrelationId++);
    if (TYPED_WRITE_NAMESPACES.has(actionType)) {
      return this.send(typedWriteRequest(actionType, payload, correlationId));
    }
    return this.send({
      type: "dispatch",
      action_type: actionType,
      payload,
      correlation_id: correlationId,
    });
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

  private send(request: WorkerRequest): RuntimeSnapshot {
    let snapshot = this.snapshot();
    for (const event of this.runtime.handle(request)) {
      snapshot = this.record(event);
    }
    return snapshot;
  }
}
