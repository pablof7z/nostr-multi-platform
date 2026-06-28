import type { IdentityRelayPermission, RuntimeStatus, WorkerEvent } from "@nmp/runtime-web";
import type { ChirpAction, RuntimeCommand } from "./actions";
import type { ChirpRelayStartOverride } from "../chirpConfig";

/** Snapshot emitted after every worker event.
 *
 * `latestUpdateBytes` carries the raw FlatBuffers `UpdateFrame` bytes from the
 * most recent `update_bytes` event. Feature modules decode these into typed
 * projections. The shell stores only raw bytes and worker event history. */
export type RuntimeSnapshot = {
  status: RuntimeStatus;
  clientRuntime: "worker" | "in_process_fallback";
  events: WorkerEvent[];
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

export type SearchOpenRequest = {
  sessionId: string;
  query: string;
  scope: "notes" | "profiles" | "longform";
  targets: "user_preferred" | "app_default" | "explicit";
  relays?: string[];
  maxHits?: number;
};

export type GroupDiscoveryOpenRequest = {
  sessionId: string;
  relayUrl: string;
};

export type GroupEventsOpenRequest = {
  sessionId: string;
  relayUrl: string;
  groupId: string;
  /** Consumer-declared kind selection (issue #2187). Empty = all h-tagged
   *  group events; a chat view passes `[9, 11]`. */
  kinds: number[];
};

export type NotificationsOpenRequest = {
  sessionId: string;
  accountPubkey: string;
};

export type NotificationsMarkReadRequest = {
  sessionId: string;
  eventIds?: string[];
  allVisible?: boolean;
};

export type NmpClient = {
  snapshot(): RuntimeSnapshot;
  subscribe(listener: (snapshot: RuntimeSnapshot) => void): () => void;
  start(relays?: ChirpRelayStartOverride): Promise<RuntimeSnapshot>;
  dispatchCommand(command: RuntimeCommand): Promise<RuntimeSnapshot>;
  dispatchChirp(action: ChirpAction): Promise<RuntimeSnapshot>;
  setSigner(pubkeyHex: string, identityRelays?: IdentityRelayPermission[]): Promise<RuntimeSnapshot>;
  setLocalKeySigner(
    secretKeyBech32: string,
    identityRelays?: IdentityRelayPermission[],
  ): Promise<RuntimeSnapshot>;
  beginSign(accountPubkey: string, unsignedJson: string): void;
  refreshRoutingDecisions(): Promise<RuntimeSnapshot>;
  openSearch(request: SearchOpenRequest): Promise<RuntimeSnapshot>;
  closeSearch(sessionId: string): Promise<RuntimeSnapshot>;
  openGroupDiscovery(request: GroupDiscoveryOpenRequest): Promise<RuntimeSnapshot>;
  closeGroupDiscovery(sessionId: string): Promise<RuntimeSnapshot>;
  openGroupEvents(request: GroupEventsOpenRequest): Promise<RuntimeSnapshot>;
  closeGroupEvents(sessionId: string): Promise<RuntimeSnapshot>;
  openNotifications(request: NotificationsOpenRequest): Promise<RuntimeSnapshot>;
  closeNotifications(sessionId: string): Promise<RuntimeSnapshot>;
  markNotificationsRead(request: NotificationsMarkReadRequest): Promise<RuntimeSnapshot>;
};
