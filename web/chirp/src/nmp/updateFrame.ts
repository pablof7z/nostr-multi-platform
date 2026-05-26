import * as flatbuffers from "flatbuffers";

import { FrameKind, UpdateFrame, Value, ValueKind } from "./generated/nmp/transport";

export type DecodedUpdateFrame =
  | { type: "snapshot"; payload: unknown }
  | { type: "panic"; message: string };

export function decodeUpdateFrameBytes(bytes: Uint8Array): DecodedUpdateFrame | undefined {
  if (bytes.length === 0) {
    return undefined;
  }
  const buffer = new flatbuffers.ByteBuffer(bytes);
  if (!UpdateFrame.bufferHasIdentifier(buffer)) {
    return undefined;
  }
  const frame = UpdateFrame.getRootAsUpdateFrame(buffer);
  switch (frame.kind()) {
    case FrameKind.Snapshot: {
      const snapshot = frame.snapshot();
      const payload = snapshot?.payload();
      return payload ? { type: "snapshot", payload: valueFromFlatBuffer(payload) } : undefined;
    }
    case FrameKind.Panic:
      return { type: "panic", message: frame.panic()?.msg() ?? "actor thread died" };
    default:
      return undefined;
  }
}

function valueFromFlatBuffer(value: Value): unknown {
  switch (value.kind()) {
    case ValueKind.Null:
      return null;
    case ValueKind.Bool:
      return value.boolValue();
    case ValueKind.Int:
      return Number(value.intValue());
    case ValueKind.UInt:
      return Number(value.uintValue());
    case ValueKind.Float:
      return value.floatValue();
    case ValueKind.String:
      return value.stringValue() ?? "";
    case ValueKind.List:
      return listFromFlatBuffer(value);
    case ValueKind.Map:
      return mapFromFlatBuffer(value);
    default:
      return null;
  }
}

function listFromFlatBuffer(value: Value): unknown[] {
  const items: unknown[] = [];
  for (let index = 0; index < value.listLength(); index += 1) {
    const item = value.list(index);
    items.push(item ? valueFromFlatBuffer(item) : null);
  }
  return items;
}

function mapFromFlatBuffer(value: Value): Record<string, unknown> {
  const record: Record<string, unknown> = {};
  for (let index = 0; index < value.mapLength(); index += 1) {
    const pair = value.map(index);
    if (!pair) {
      continue;
    }
    const key = pair.key();
    if (!key) {
      continue;
    }
    const nested = pair.value();
    record[key] = nested ? valueFromFlatBuffer(nested) : null;
  }
  return record;
}
