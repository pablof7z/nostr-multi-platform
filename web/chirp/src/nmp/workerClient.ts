import {
  eventCorrelationId,
  protocolVersion,
  type IdentityRelayPermission,
  type WorkerEvent,
  type WorkerRequest,
} from "@nmp/runtime-web";
import { chirpActionRequest, type ChirpAction, type RuntimeCommand } from "./actions";
import { chirpStartRelays, type ChirpRelayStartOverride } from "../chirpConfig";
import { fulfilSignRequestViaExtension } from "./signBroker";
import { makeCorrelationId } from "./correlationId";
import { BaseClient } from "./clientBase";
import {
  runtimeConnection,
  type GroupDiscoveryOpenRequest,
  type GroupEventsOpenRequest,
  type NotificationsMarkReadRequest,
  type NotificationsOpenRequest,
  type RuntimeSnapshot,
  type SearchOpenRequest,
} from "./clientTypes";

export class WorkerNmpClient extends BaseClient {
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
      case "dispatch_bytes":
        return this.request(
          { type: "dispatch_bytes", bytes: command.buildDispatchBytes(correlationId) },
          correlationId,
        );
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
      case "relay_config":
        return this.request(
          {
            type: "relay_config",
            action: command.action,
            url: command.url,
            role: command.role,
            correlation_id: correlationId,
          },
          correlationId,
        );
      case "publish_relay_preferences":
        return this.request(
          { type: "publish_relay_preferences", correlation_id: correlationId },
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

  async openSearch(request: SearchOpenRequest): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-search", this.nextCorrelationId++);
    return this.request(
      {
        type: "search_open",
        session_id: request.sessionId,
        query: request.query,
        scope: request.scope,
        targets: request.targets,
        relays: request.relays ?? [],
        max_hits: request.maxHits,
        correlation_id: correlationId,
      },
      correlationId,
    );
  }

  async closeSearch(sessionId: string): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-search", this.nextCorrelationId++);
    return this.request(
      { type: "search_close", session_id: sessionId, correlation_id: correlationId },
      correlationId,
    );
  }

  async openGroupDiscovery(request: GroupDiscoveryOpenRequest): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-groups", this.nextCorrelationId++);
    return this.request(
      {
        type: "group_discovery_open",
        session_id: request.sessionId,
        relay_url: request.relayUrl,
        correlation_id: correlationId,
      },
      correlationId,
    );
  }

  async closeGroupDiscovery(sessionId: string): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-groups", this.nextCorrelationId++);
    return this.request(
      { type: "group_discovery_close", session_id: sessionId, correlation_id: correlationId },
      correlationId,
    );
  }

  async openGroupEvents(request: GroupEventsOpenRequest): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-group-events", this.nextCorrelationId++);
    return this.request(
      {
        type: "group_events_open",
        session_id: request.sessionId,
        relay_url: request.relayUrl,
        group_id: request.groupId,
        kinds: request.kinds,
        correlation_id: correlationId,
      },
      correlationId,
    );
  }

  async closeGroupEvents(sessionId: string): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-group-events", this.nextCorrelationId++);
    return this.request(
      { type: "group_events_close", session_id: sessionId, correlation_id: correlationId },
      correlationId,
    );
  }

  async openNotifications(request: NotificationsOpenRequest): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-notifications", this.nextCorrelationId++);
    return this.request(
      {
        type: "notifications_open",
        session_id: request.sessionId,
        account_pubkey: request.accountPubkey,
        correlation_id: correlationId,
      },
      correlationId,
    );
  }

  async closeNotifications(sessionId: string): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-notifications", this.nextCorrelationId++);
    return this.request(
      { type: "notifications_close", session_id: sessionId, correlation_id: correlationId },
      correlationId,
    );
  }

  async markNotificationsRead(request: NotificationsMarkReadRequest): Promise<RuntimeSnapshot> {
    await this.helloReady;
    const correlationId = makeCorrelationId("web-notifications", this.nextCorrelationId++);
    return this.request({
      type: "notifications_mark_read",
      session_id: request.sessionId,
      event_ids: request.eventIds ?? [],
      all_visible: request.allVisible ?? false,
      correlation_id: correlationId,
    }, correlationId);
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
    if (event.type === "sign_request") {
      const actionCorrelationId = event.action_correlation_id ?? undefined;
      if (actionCorrelationId) {
        const resolve = this.pending.get(actionCorrelationId);
        if (resolve) {
          this.pending.delete(actionCorrelationId);
          resolve(snapshot);
        }
      }
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
