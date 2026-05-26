import * as flatbuffers from "flatbuffers";

import { FrameKind, UpdateFrame, Value, ValueKind } from "./generated/nmp/transport";

export const SNAPSHOT_SCHEMA_VERSION = 1;

export type UpdateFrameDecodeErrorKind =
  | "invalid_flatbuffer"
  | "invalid_value"
  | "missing_snapshot_payload"
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

export type DecodedUpdateFrame =
  | { type: "snapshot"; schemaVersion: number; payload: unknown }
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
          "missing_snapshot_payload",
          "snapshot frame missing payload",
        );
      }
      const payload = snapshot.payload();
      if (!payload) {
        throw new UpdateFrameDecodeError(
          "missing_snapshot_payload",
          "snapshot frame missing payload",
        );
      }
      return {
        type: "snapshot",
        schemaVersion: snapshot.schemaVersion(),
        payload: valueFromFlatBuffer(payload),
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

function valueFromFlatBuffer(value: Value): unknown {
  switch (value.kind()) {
    case ValueKind.Null:
      return null;
    case ValueKind.Bool:
      return value.boolValue();
    case ValueKind.Int:
      return narrowBigInt(value.intValue());
    case ValueKind.UInt:
      return narrowBigInt(value.uintValue());
    case ValueKind.Float: {
      const float = value.floatValue();
      if (!Number.isFinite(float)) {
        throw new UpdateFrameDecodeError("invalid_value", "non-finite float value");
      }
      return float;
    }
    case ValueKind.String: {
      const string = value.stringValue();
      if (string === null) {
        throw new UpdateFrameDecodeError(
          "invalid_value",
          "string value missing string_value",
        );
      }
      return string;
    }
    case ValueKind.List:
      return listFromFlatBuffer(value);
    case ValueKind.Map:
      return mapFromFlatBuffer(value);
    default:
      throw new UpdateFrameDecodeError(
        "invalid_value",
        `unknown value kind ${value.kind()}`,
      );
  }
}

function listFromFlatBuffer(value: Value): unknown[] {
  const items: unknown[] = [];
  for (let index = 0; index < value.listLength(); index += 1) {
    const item = value.list(index);
    if (!item) {
      throw new UpdateFrameDecodeError(
        "invalid_value",
        `list item at index ${index} missing value`,
      );
    }
    items.push(valueFromFlatBuffer(item));
  }
  return items;
}

function mapFromFlatBuffer(value: Value): Record<string, unknown> {
  const record: Record<string, unknown> = {};
  for (let index = 0; index < value.mapLength(); index += 1) {
    const pair = value.map(index);
    if (!pair) {
      throw new UpdateFrameDecodeError(
        "invalid_value",
        `map pair at index ${index} missing pair`,
      );
    }
    const key = pair.key();
    if (key === null) {
      throw new UpdateFrameDecodeError(
        "invalid_value",
        `map pair at index ${index} missing key`,
      );
    }
    const nested = pair.value();
    if (!nested) {
      throw new UpdateFrameDecodeError(
        "invalid_value",
        `map pair at index ${index} missing value`,
      );
    }
    record[key] = valueFromFlatBuffer(nested);
  }
  return record;
}

// Stay in `number` while the value fits IEEE-754 integer precision; promote
// to BigInt only past 2^53 to preserve precision for u64 counters
// (`bytes_rx`, ms-since-epoch) without forcing every small integer consumer
// onto BigInt.
function narrowBigInt(value: bigint): number | bigint {
  if (
    value >= BigInt(Number.MIN_SAFE_INTEGER) &&
    value <= BigInt(Number.MAX_SAFE_INTEGER)
  ) {
    return Number(value);
  }
  return value;
}
