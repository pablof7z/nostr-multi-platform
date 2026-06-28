import {
  DegradedRuntime,
  protocolVersion,
  type IdentityRelayPermission,
  type WorkerRequest,
} from "@nmp/runtime-web";
import { chirpActionRequest, type ChirpAction, type RuntimeCommand } from "./actions";
import { chirpStartRelays, type ChirpRelayStartOverride } from "../chirpConfig";
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

export class InProcessNmpClient extends BaseClient {
  private readonly runtime = new DegradedRuntime(
    "browser_bridge_unavailable",
    "Web Worker support is unavailable, so the nmp-browser-runtime bridge cannot start",
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
      case "dispatch_bytes":
        return this.send({ type: "dispatch_bytes", bytes: command.buildDispatchBytes(correlationId) });
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
      case "relay_config":
        return this.send({
          type: "relay_config",
          action: command.action,
          url: command.url,
          role: command.role,
          correlation_id: correlationId,
        });
      case "publish_relay_preferences":
        return this.send({ type: "publish_relay_preferences", correlation_id: correlationId });
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

  async openSearch(request: SearchOpenRequest): Promise<RuntimeSnapshot> {
    return this.send({
      type: "search_open",
      session_id: request.sessionId,
      query: request.query,
      scope: request.scope,
      targets: request.targets,
      relays: request.relays ?? [],
      max_hits: request.maxHits,
      correlation_id: makeCorrelationId("web-search", this.nextCorrelationId++),
    });
  }

  async closeSearch(sessionId: string): Promise<RuntimeSnapshot> {
    return this.send({
      type: "search_close",
      session_id: sessionId,
      correlation_id: makeCorrelationId("web-search", this.nextCorrelationId++),
    });
  }

  async openGroupDiscovery(request: GroupDiscoveryOpenRequest): Promise<RuntimeSnapshot> {
    return this.send({
      type: "group_discovery_open",
      session_id: request.sessionId,
      relay_url: request.relayUrl,
      correlation_id: makeCorrelationId("web-groups", this.nextCorrelationId++),
    });
  }

  async closeGroupDiscovery(sessionId: string): Promise<RuntimeSnapshot> {
    return this.send({
      type: "group_discovery_close",
      session_id: sessionId,
      correlation_id: makeCorrelationId("web-groups", this.nextCorrelationId++),
    });
  }

  async openGroupEvents(request: GroupEventsOpenRequest): Promise<RuntimeSnapshot> {
    return this.send({
      type: "group_events_open",
      session_id: request.sessionId,
      relay_url: request.relayUrl,
      group_id: request.groupId,
      kinds: request.kinds,
      correlation_id: makeCorrelationId("web-group-events", this.nextCorrelationId++),
    });
  }

  async closeGroupEvents(sessionId: string): Promise<RuntimeSnapshot> {
    return this.send({
      type: "group_events_close",
      session_id: sessionId,
      correlation_id: makeCorrelationId("web-group-events", this.nextCorrelationId++),
    });
  }

  async openNotifications(request: NotificationsOpenRequest): Promise<RuntimeSnapshot> {
    return this.send({
      type: "notifications_open",
      session_id: request.sessionId,
      account_pubkey: request.accountPubkey,
      correlation_id: makeCorrelationId("web-notifications", this.nextCorrelationId++),
    });
  }

  async closeNotifications(sessionId: string): Promise<RuntimeSnapshot> {
    return this.send({
      type: "notifications_close",
      session_id: sessionId,
      correlation_id: makeCorrelationId("web-notifications", this.nextCorrelationId++),
    });
  }

  async markNotificationsRead(request: NotificationsMarkReadRequest): Promise<RuntimeSnapshot> {
    return this.send({
      type: "notifications_mark_read",
      session_id: request.sessionId,
      event_ids: request.eventIds ?? [],
      all_visible: request.allVisible ?? false,
      correlation_id: makeCorrelationId("web-notifications", this.nextCorrelationId++),
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
