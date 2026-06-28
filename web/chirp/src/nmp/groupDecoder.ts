import * as flatbuffers from "flatbuffers";
import { DiscoveredGroup } from "./generated/nmp/nip29/discovered-group";
import { DiscoveredGroupsSnapshot } from "./generated/nmp/nip29/discovered-groups-snapshot";
import { FrameKind } from "./generated/nmp/transport/frame-kind";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";

const GROUPS_KEY = "nmp.nip29.discovered_groups";
const GROUPS_FILE_ID = "NDGS";

export type DiscoveredGroupRow = {
  groupId: string;
  hostRelayUrl: string;
  name?: string;
  picture?: string;
  about?: string;
  memberCount: number;
  adminCount: number;
  public: boolean;
  open: boolean;
  parent?: string;
  children: string[];
};

export type GroupDiscoveryFrame = {
  hostRelayUrl: string;
  rows: DiscoveredGroupRow[];
};

export function decodeGroupDiscoveryFrame(
  bytes: Uint8Array | undefined,
): GroupDiscoveryFrame | undefined {
  if (!bytes) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!UpdateFrame.bufferHasIdentifier(bb)) return undefined;
    const frame = UpdateFrame.getRootAsUpdateFrame(bb);
    if (frame.kind() !== FrameKind.Snapshot) return undefined;
    const snap = frame.snapshot();
    if (!snap) return undefined;
    for (let i = 0; i < snap.typedProjectionsLength(); i++) {
      const projection = snap.typedProjections(i);
      const payload = projection?.payload();
      const payloadBytes = payload?.payloadArray();
      if (
        projection?.key() !== GROUPS_KEY ||
        payload?.fileIdentifier() !== GROUPS_FILE_ID ||
        !payloadBytes ||
        payloadBytes.length === 0
      ) {
        continue;
      }
      return decodePayload(payloadBytes);
    }
    return undefined;
  } catch {
    return undefined;
  }
}

function decodePayload(bytes: Uint8Array): GroupDiscoveryFrame | undefined {
  const bb = new flatbuffers.ByteBuffer(bytes);
  if (!DiscoveredGroupsSnapshot.bufferHasIdentifier(bb)) return undefined;
  const snapshot = DiscoveredGroupsSnapshot.getRootAsDiscoveredGroupsSnapshot(bb);
  const rows: DiscoveredGroupRow[] = [];
  for (let i = 0; i < snapshot.groupsLength(); i++) {
    const group = snapshot.groups(i);
    const row = group ? decodeGroup(group) : undefined;
    if (row) rows.push(row);
  }
  return {
    hostRelayUrl: snapshot.hostRelayUrl() ?? "",
    rows,
  };
}

function decodeGroup(group: DiscoveredGroup): DiscoveredGroupRow | undefined {
  const groupId = group.groupId();
  const hostRelayUrl = group.hostRelayUrl();
  if (!groupId || !hostRelayUrl) return undefined;
  const children: string[] = [];
  for (let i = 0; i < group.childrenLength(); i++) {
    const child = group.children(i);
    if (typeof child === "string") children.push(child);
  }
  return {
    groupId,
    hostRelayUrl,
    name: group.name() ?? undefined,
    picture: group.picture() ?? undefined,
    about: group.about() ?? undefined,
    memberCount: group.memberCount(),
    adminCount: group.adminCount(),
    public: group.public_(),
    open: group.open(),
    parent: group.parent() ?? undefined,
    children,
  };
}
