import * as flatbuffers from "flatbuffers";

import { FrameKind, RelayStatus, UpdateFrame } from "./generated/nmp/transport";

export const SNAPSHOT_SCHEMA_VERSION = 1;

export type UpdateFrameDecodeErrorKind =
  | "invalid_flatbuffer"
  | "missing_panic_payload"
  | "unexpected_panic_frame"
  | "schema_version_mismatch";

export class UpdateFrameDecodeError extends Error {
  constructor(
    public readonly kind: UpdateFrameDecodeErrorKind,
    message: string,
  ) {
    super(message);
    this.name = "UpdateFrameDecodeError";
  }
}

/** Relay status decoded from the SnapshotFrame Tier-3 `relay_statuses` field. */
export type DecodedRelayStatus = {
  /** WebSocket URL of the relay, e.g. `wss://relay.example`. */
  url: string;
  /** Role string as emitted by the kernel, e.g. `"both"`, `"read"`, `"write"`. */
  role: string;
  /** Connection state string, e.g. `"Connected"`, `"Connecting"`, `"Disconnected"`. */
  status: string;
};

export type DecodedUpdateFrame =
  | {
      type: "snapshot";
      schemaVersion: number;
      /** True when the kernel's run state is Running. */
      running: boolean;
      /** Per-relay status rows decoded from the Tier-3 `relay_statuses` field. */
      relayStatuses: DecodedRelayStatus[];
    }
  | { type: "panic"; message: string };

export function decodeUpdateFrameBytes(bytes: Uint8Array): DecodedUpdateFrame {
  if (bytes.length === 0) {
    throw new UpdateFrameDecodeError("invalid_flatbuffer", "empty update frame buffer");
  }
  const buffer = new flatbuffers.ByteBuffer(bytes);
  if (!UpdateFrame.bufferHasIdentifier(buffer)) {
    throw new UpdateFrameDecodeError(
      "invalid_flatbuffer",
      "missing NMPU file identifier",
    );
  }
  const frame = UpdateFrame.getRootAsUpdateFrame(buffer);
  switch (frame.kind()) {
    case FrameKind.Snapshot: {
      const snapshot = frame.snapshot();
      if (!snapshot) {
        throw new UpdateFrameDecodeError(
          "invalid_flatbuffer",
          "snapshot frame table is absent",
        );
      }
      const relayStatuses: DecodedRelayStatus[] = [];
      for (let i = 0; i < snapshot.relayStatusesLength(); i += 1) {
        const relay = snapshot.relayStatuses(i, new RelayStatus());
        if (relay) {
          relayStatuses.push({
            url: relay.relayUrl() ?? "",
            role: relay.role() ?? "",
            status: relay.connection() ?? "",
          });
        }
      }
      return {
        type: "snapshot",
        schemaVersion: snapshot.schemaVersion(),
        running: snapshot.running(),
        relayStatuses,
      };
    }
    case FrameKind.Panic: {
      const panic = frame.panic();
      if (!panic) {
        throw new UpdateFrameDecodeError(
          "missing_panic_payload",
          "panic frame missing payload",
        );
      }
      const message = panic.msg();
      if (message === null) {
        throw new UpdateFrameDecodeError(
          "missing_panic_payload",
          "panic frame missing msg",
        );
      }
      return { type: "panic", message };
    }
    default:
      throw new UpdateFrameDecodeError(
        "invalid_flatbuffer",
        `unknown frame kind ${frame.kind()}`,
      );
  }
}
