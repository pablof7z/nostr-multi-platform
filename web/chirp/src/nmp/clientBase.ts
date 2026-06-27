import type { IdentityRelayPermission, RuntimeStatus, WorkerEvent } from "@nmp/runtime-web";
import type { ChirpAction, RuntimeCommand } from "./actions";
import type { ChirpRelayStartOverride } from "../chirpConfig";
import type {
  GroupDiscoveryOpenRequest,
  GroupEventsOpenRequest,
  NmpClient,
  NotificationsMarkReadRequest,
  NotificationsOpenRequest,
  RuntimeSnapshot,
  SearchOpenRequest,
} from "./clientTypes";

export abstract class BaseClient implements NmpClient {
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
      const bytes = event.bytes instanceof Uint8Array ? event.bytes : new Uint8Array(event.bytes);
      this.latestUpdateBytes = bytes;
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
  abstract openSearch(request: SearchOpenRequest): Promise<RuntimeSnapshot>;
  abstract closeSearch(sessionId: string): Promise<RuntimeSnapshot>;
  abstract openGroupDiscovery(request: GroupDiscoveryOpenRequest): Promise<RuntimeSnapshot>;
  abstract closeGroupDiscovery(sessionId: string): Promise<RuntimeSnapshot>;
  abstract openGroupEvents(request: GroupEventsOpenRequest): Promise<RuntimeSnapshot>;
  abstract closeGroupEvents(sessionId: string): Promise<RuntimeSnapshot>;
  abstract openNotifications(request: NotificationsOpenRequest): Promise<RuntimeSnapshot>;
  abstract closeNotifications(sessionId: string): Promise<RuntimeSnapshot>;
  abstract markNotificationsRead(request: NotificationsMarkReadRequest): Promise<RuntimeSnapshot>;
}
