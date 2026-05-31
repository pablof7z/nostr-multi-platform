import { afterEach, describe, expect, it, vi } from "vitest";
import goldenSnapshotV1Hex from "../../../../crates/nmp-core/tests/fixtures/update_frame_snapshot_v1.fb.hex?raw";
import {
  discoverGroupsCommand,
  publishNoteAction,
  reactGroupMessageCommand,
  replyGroupMessageCommand,
  sendDmCommand,
  walletCommand,
} from "./actions";
import { createNmpClient, type RuntimeSnapshot } from "./client";
import { DegradedRuntime } from "./degradedRuntime";
import * as flatbuffers from "flatbuffers";
import type { WorkerEvent, WorkerRequest } from "./protocol";
import { eventCorrelationId, protocolVersion } from "./protocol";
import { chirpTimelineFromEnvelope, displayRows, featureSnapshotFromEnvelope, kernelSnapshotFromEnvelope } from "./snapshot";
import { FrameKind, Pair, PanicFrame, SnapshotFrame, UpdateFrame, Value, ValueKind } from "./generated/nmp/transport";
import { decodeUpdateFrameBytes, UpdateFrameDecodeError } from "./updateFrame";

type WorkerHarness = {
  onmessage: ((message: MessageEvent<WorkerRequest>) => void) | null;
  postMessage: (event: WorkerEvent) => void;
};

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe("DegradedRuntime protocol flow", () => {
  it("accepts hello, degrades on start, and rejects dispatch capabilities", () => {
    const runtime = new DegradedRuntime();

    expect(
      runtime.handle({
        type: "hello",
        app_id: "chirp",
        platform: "web",
        protocol_version: protocolVersion,
      }),
    ).toEqual([{ type: "hello_accepted", protocol_version: 1, status: "ready" }]);

    expect(
      runtime.handle({
        type: "start",
        app_id: "chirp",
        database_name: "chirp-test",
        correlation_id: "start-1",
      }),
    ).toEqual([
      {
        type: "runtime_status",
        status: { degraded: "browser_actor_driver_missing" },
        correlation_id: "start-1",
      },
    ]);

    expect(
      runtime.handle({
        type: "dispatch",
        action_type: "nmp.publish",
        payload: { PublishNote: { content: "hello", reply_to_id: null, target: "Auto" } },
        correlation_id: "dispatch-1",
      }),
    ).toEqual([
      {
        type: "capability_failure",
        capability: "nmp.publish",
        correlation_id: "dispatch-1",
        reason: "nmp-wasm actor driver is not linked into the web worker yet",
      },
    ]);
  });

  it("returns a protocol mismatch error for incompatible hello requests", () => {
    const runtime = new DegradedRuntime();

    expect(
      runtime.handle({
        type: "hello",
        app_id: "chirp",
        platform: "web",
        protocol_version: protocolVersion + 1,
      }),
    ).toEqual([
      {
        type: "error",
        code: "protocol_mismatch",
        message: "expected protocol 1, got 2",
      },
    ]);
  });
});

describe("createNmpClient fallback", () => {
  it("uses the degraded runtime when Worker is unavailable", async () => {
    vi.stubGlobal("Worker", undefined);

    const client = createNmpClient();
    const snapshots = [client.snapshot()];

    const unsubscribe = client.subscribe((snapshot) => snapshots.push(snapshot));
    const started = await client.start();
    const dispatched = await client.dispatchChirp(publishNoteAction("hello"));
    unsubscribe();

    expect(snapshots[0].events[0]).toMatchObject({ type: "hello_accepted" });
    expect(started.status).toEqual({ degraded: "browser_bridge_unavailable" });
    expect(started.events[0]).toMatchObject({
      type: "runtime_status",
      correlation_id: "web-start",
    });
    expect(dispatched.events[0]).toMatchObject({
      type: "capability_failure",
      capability: "chirp_action",
      reason: "Web Worker support is unavailable, so the nmp-wasm bridge cannot start",
    });
  });
});

describe("shared Chirp web semantics", () => {
  it("treats binary update events as out-of-band snapshot transport", () => {
    expect(eventCorrelationId({ type: "update_bytes", bytes: new Uint8Array([1, 2, 3]) })).toBeUndefined();
  });

  it("decodes generated FlatBuffers snapshot updates without JSON fallback", () => {
    const bytes = makeSnapshotBytes({ rev: 7, running: true });
    const decoded = decodeUpdateFrameBytes(bytes);

    expect(decoded).toEqual({
      type: "snapshot",
      schemaVersion: 1,
      payload: { rev: 7, running: true },
    });
  });

  it("matches the Rust golden snapshot v1 fixture byte-for-byte", () => {
    const hex = goldenSnapshotV1Hex.replace(/\s+/g, "");
    if (hex.length % 2 !== 0) {
      throw new Error("hex fixture must contain full bytes");
    }
    const bytes = new Uint8Array(hex.length / 2);
    for (let i = 0; i < bytes.length; i += 1) {
      bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
    }

    const decoded = decodeUpdateFrameBytes(bytes);
    expect(decoded).toEqual({
      type: "snapshot",
      schemaVersion: 1,
      payload: {
        schema_version: 1,
        rev: 42,
        running: true,
        projections: { timeline: [{ id: "a", score: 1.5 }] },
      },
    });
  });

  it("promotes integers above Number.MAX_SAFE_INTEGER to BigInt", () => {
    const huge = BigInt(Number.MAX_SAFE_INTEGER) + 10n;
    const bytes = encodeRawValue((builder) =>
      Value.createValue(builder, ValueKind.UInt, false, BigInt(0), huge, 0, 0, 0, 0),
    );

    const decoded = decodeUpdateFrameBytes(bytes);
    expect(decoded).toEqual({ type: "snapshot", schemaVersion: 1, payload: huge });
  });

  it("rejects NaN floats instead of degrading to null", () => {
    const bytes = encodeRawValue((builder) =>
      Value.createValue(
        builder,
        ValueKind.Float,
        false,
        BigInt(0),
        BigInt(0),
        Number.NaN,
        0,
        0,
        0,
      ),
    );

    let caught: unknown;
    try {
      decodeUpdateFrameBytes(bytes);
    } catch (error) {
      caught = error;
    }
    expect(caught).toBeInstanceOf(UpdateFrameDecodeError);
    expect((caught as UpdateFrameDecodeError).kind).toBe("invalid_value");
  });

  it("throws on unknown value kinds instead of returning null", () => {
    const bytes = encodeRawValue((builder) =>
      Value.createValue(builder, 99 as ValueKind, false, BigInt(0), BigInt(0), 0, 0, 0, 0),
    );

    let caught: unknown;
    try {
      decodeUpdateFrameBytes(bytes);
    } catch (error) {
      caught = error;
    }
    expect(caught).toBeInstanceOf(UpdateFrameDecodeError);
    expect((caught as UpdateFrameDecodeError).kind).toBe("invalid_value");
  });

  it("throws on buffers missing the NMPU identifier", () => {
    let caught: unknown;
    try {
      decodeUpdateFrameBytes(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]));
    } catch (error) {
      caught = error;
    }
    expect(caught).toBeInstanceOf(UpdateFrameDecodeError);
    expect((caught as UpdateFrameDecodeError).kind).toBe("invalid_flatbuffer");
  });

  it("throws missing_panic_payload on a panic frame with no msg offset", () => {
    const builder = new flatbuffers.Builder(64);
    PanicFrame.startPanicFrame(builder);
    const panicOffset = builder.endObject();
    UpdateFrame.startUpdateFrame(builder);
    UpdateFrame.addKind(builder, FrameKind.Panic);
    UpdateFrame.addPanic(builder, panicOffset);
    const frameOffset = UpdateFrame.endUpdateFrame(builder);
    UpdateFrame.finishUpdateFrameBuffer(builder, frameOffset);

    let caught: unknown;
    try {
      decodeUpdateFrameBytes(builder.asUint8Array());
    } catch (error) {
      caught = error;
    }
    expect(caught).toBeInstanceOf(UpdateFrameDecodeError);
    expect((caught as UpdateFrameDecodeError).kind).toBe("missing_panic_payload");
  });

  it("sends a Chirp intent and lets Rust map it to the kernel publish action", () => {
    expect(publishNoteAction("hello web")).toEqual({
      action: "publish_note",
      content: "hello web",
      reply_to_id: null,
    });
  });

  it("uses the shared action namespaces for non-home Chirp features", () => {
    expect(sendDmCommand("pk", "hello")).toEqual({
      actionType: "nmp.nip17.send",
      payload: { recipient_pubkey: "pk", content: "hello" },
    });
    expect(discoverGroupsCommand("wss://groups.example")).toEqual({
      actionType: "nmp.nip29.discover",
      payload: { relay_url: "wss://groups.example" },
    });
    expect(walletCommand("pay_invoice", { bolt11: "lnbc1..." })).toEqual({
      actionType: "nmp.wallet.pay_invoice",
      payload: { bolt11: "lnbc1..." },
    });
    expect(reactGroupMessageCommand("wss://groups.example", "general", "event1")).toEqual({
      actionType: "nmp.nip29.react_in_group",
      payload: {
        group: { host_relay_url: "wss://groups.example", local_id: "general" },
        target_event_id: "event1",
        content: "+",
      },
    });
    expect(replyGroupMessageCommand("wss://groups.example", "general", "event1", "reply")).toMatchObject({
      actionType: "nmp.nip29.comment_in_group",
      payload: { parent_event_id: "event1", content: "reply" },
    });
  });

  it("renders rows from Rust snapshot envelopes instead of local feed fixtures", () => {
    const kernel = kernelSnapshotFromEnvelope({
      t: "snapshot",
      v: {
        rev: 7,
        projections: {
          timeline: [
            {
              id: "note-a",
              displayName: "alice",
              content: "from shared timeline",
            },
          ],
        },
      },
    });

    expect(kernel?.rev).toBe(7);
    expect(displayRows(kernel, undefined)).toEqual([
      {
        id: "note-a",
        displayName: "alice",
        content: "from shared timeline",
      },
    ]);
  });

  it("can fall back to the Chirp modular snapshot card shape", () => {
    const chirp = chirpTimelineFromEnvelope({
      chirpTimeline: {
        blocks: [{ Standalone: "note-a" }],
        cards: [{ id: "note-a", author_pubkey: "abc", content: "from chirp cards" }],
      },
    });

    expect(displayRows(undefined, chirp)).toEqual([
      {
        id: "note-a",
        authorPubkey: "abc",
        content: "from chirp cards",
        createdAt: undefined,
      },
    ]);
  });

  it("projects iOS/TUI parity feature snapshots from shared Rust projections", () => {
    const feature = featureSnapshotFromEnvelope({
      t: "snapshot",
      v: {
        projections: {
          accounts: [{ id: "alice", display_name: "Alice", npub: "npub1alice", is_active: true }],
          active_account: "alice",
          relay_edit_rows: [{ url: "wss://relay.example", role: "both" }],
          relay_diagnostics: [{ url: "wss://relay.example", role: "both,indexer", status: "configured" }],
          wallet: { status: "ready", balance_msats: 21000 },
          "nmp.nip17.dm_inbox": {
            conversations: [{ peer_pubkey: "bob", messages: [{ id: "dm1", content: "hi", is_outgoing: false }] }],
          },
          "nmp.nip29.discovered_groups": {
            groups: [{ host_relay_url: "wss://groups.example", group_id: "general", member_count: 3 }],
          },
          publish_outbox: [{ handle: "pub1", status_label: "pending", can_retry: true }],
        },
      },
    });

    expect(feature.accounts[0]).toMatchObject({ id: "alice", display: "Alice", active: true });
    expect(feature.dmConversations[0].latest).toBe("hi");
    expect(feature.discoveredGroups[0]).toMatchObject({ groupId: "general", memberCount: 3 });
    expect(feature.relayDiagnostics[0].status).toBe("configured");
    expect(feature.wallet.balanceMsats).toBe(21000);
    expect(feature.outbox[0].canRetry).toBe(true);
  });
});

describe("client schema enforcement", () => {
  it("degrades to protocol_mismatch when the frame schema version diverges", async () => {
    const stub = new StubWorker();
    vi.stubGlobal("Worker", StubWorker.factory(stub));

    const client = createNmpClient();
    const snapshots: RuntimeSnapshot[] = [];
    const unsubscribe = client.subscribe((snapshot) => snapshots.push(snapshot));

    stub.emit({
      type: "update_bytes",
      bytes: makeSnapshotBytes({ schema_version: 1, rev: 1 }, 2),
    });

    unsubscribe();
    expect(snapshots[snapshots.length - 1]!.status).toEqual({ degraded: "protocol_mismatch" });
    expect(snapshots[snapshots.length - 1]!.latestUpdate).toBeUndefined();
  });

  it("degrades to protocol_mismatch when the payload schema_version diverges", async () => {
    const stub = new StubWorker();
    vi.stubGlobal("Worker", StubWorker.factory(stub));

    const client = createNmpClient();
    const snapshots: RuntimeSnapshot[] = [];
    const unsubscribe = client.subscribe((snapshot) => snapshots.push(snapshot));

    stub.emit({
      type: "update_bytes",
      bytes: makeSnapshotBytes({ schema_version: 2, rev: 1 }, 1),
    });

    unsubscribe();
    expect(snapshots[snapshots.length - 1]!.status).toEqual({ degraded: "protocol_mismatch" });
  });

  it("degrades to browser_actor_driver_missing on malformed update bytes", async () => {
    const stub = new StubWorker();
    vi.stubGlobal("Worker", StubWorker.factory(stub));

    const client = createNmpClient();
    const snapshots: RuntimeSnapshot[] = [];
    const unsubscribe = client.subscribe((snapshot) => snapshots.push(snapshot));

    stub.emit({ type: "update_bytes", bytes: new Uint8Array([0, 0, 0, 0, 1, 2, 3, 4]) });

    unsubscribe();
    expect(snapshots[snapshots.length - 1]!.status).toEqual({
      degraded: "browser_actor_driver_missing",
    });
  });
});

describe("worker runtime bridge", () => {
  it("reports unavailable wasm bridge and posts degraded fallback events", async () => {
    const events: WorkerEvent[] = [];
    const harness: WorkerHarness = {
      onmessage: null,
      postMessage: (event) => events.push(event),
    };
    vi.stubGlobal("location", { origin: "http://localhost" });
    vi.stubGlobal("self", harness);

    await import("./worker");

    await sendWorkerRequest(harness, {
      type: "hello",
      app_id: "chirp",
      platform: "web",
      protocol_version: protocolVersion,
    });
    await sendWorkerRequest(harness, {
      type: "start",
      app_id: "chirp",
      database_name: "chirp-test",
      correlation_id: "start-1",
    });
    await sendWorkerRequest(harness, {
      type: "chirp_action",
      action: publishNoteAction("hello"),
      correlation_id: "dispatch-1",
    });

    expect(events[0]).toMatchObject({
      type: "error",
      code: "wasm_bridge_unavailable",
    });
    expect(events.slice(1)).toEqual([
      { type: "hello_accepted", protocol_version: 1, status: "ready" },
      {
        type: "runtime_status",
        status: { degraded: "browser_bridge_unavailable" },
        correlation_id: "start-1",
      },
      {
        type: "capability_failure",
        capability: "chirp_action",
        correlation_id: "dispatch-1",
        reason: events[0].type === "error" ? events[0].message : "",
      },
    ]);
  });
});

async function sendWorkerRequest(harness: WorkerHarness, request: WorkerRequest) {
  if (!harness.onmessage) {
    throw new Error("worker did not register an onmessage handler");
  }
  await harness.onmessage({ data: request } as MessageEvent<WorkerRequest>);
}

function makeSnapshotBytes(
  payload: Record<string, unknown>,
  schemaVersion = 1,
): Uint8Array {
  const builder = new flatbuffers.Builder(128);
  const payloadOffset = buildValue(builder, payload);
  SnapshotFrame.startSnapshotFrame(builder);
  SnapshotFrame.addSchemaVersion(builder, schemaVersion);
  SnapshotFrame.addPayload(builder, payloadOffset);
  const snapshotOffset = SnapshotFrame.endSnapshotFrame(builder);
  UpdateFrame.startUpdateFrame(builder);
  UpdateFrame.addKind(builder, FrameKind.Snapshot);
  UpdateFrame.addSnapshot(builder, snapshotOffset);
  const frameOffset = UpdateFrame.endUpdateFrame(builder);
  UpdateFrame.finishUpdateFrameBuffer(builder, frameOffset);
  return builder.asUint8Array();
}

function encodeRawValue(
  build: (builder: flatbuffers.Builder) => flatbuffers.Offset,
): Uint8Array {
  const builder = new flatbuffers.Builder(64);
  const payloadOffset = build(builder);
  SnapshotFrame.startSnapshotFrame(builder);
  SnapshotFrame.addSchemaVersion(builder, 1);
  SnapshotFrame.addPayload(builder, payloadOffset);
  const snapshotOffset = SnapshotFrame.endSnapshotFrame(builder);
  UpdateFrame.startUpdateFrame(builder);
  UpdateFrame.addKind(builder, FrameKind.Snapshot);
  UpdateFrame.addSnapshot(builder, snapshotOffset);
  const frameOffset = UpdateFrame.endUpdateFrame(builder);
  UpdateFrame.finishUpdateFrameBuffer(builder, frameOffset);
  return builder.asUint8Array();
}

function buildValue(builder: flatbuffers.Builder, value: unknown): flatbuffers.Offset {
  if (typeof value === "boolean") {
    return Value.createValue(builder, ValueKind.Bool, value, BigInt(0), BigInt(0), 0, 0, 0, 0);
  }
  if (typeof value === "number") {
    return Value.createValue(builder, ValueKind.UInt, false, BigInt(0), BigInt(value), 0, 0, 0, 0);
  }
  if (typeof value === "object" && value !== null && !Array.isArray(value)) {
    const pairs = Object.entries(value).map(([key, nested]) => {
      const nestedOffset = buildValue(builder, nested);
      const keyOffset = builder.createString(key);
      Pair.startPair(builder);
      Pair.addKey(builder, keyOffset);
      Pair.addValue(builder, nestedOffset);
      return Pair.endPair(builder);
    });
    const mapOffset = Value.createMapVector(builder, pairs);
    return Value.createValue(builder, ValueKind.Map, false, BigInt(0), BigInt(0), 0, 0, 0, mapOffset);
  }
  return Value.createValue(builder, ValueKind.Null, false, BigInt(0), BigInt(0), 0, 0, 0, 0);
}

class StubWorker {
  onmessage: ((message: MessageEvent<WorkerEvent>) => void) | null = null;
  postMessage(_request: WorkerRequest): void {}
  terminate(): void {}

  emit(event: WorkerEvent): void {
    this.onmessage?.({ data: event } as MessageEvent<WorkerEvent>);
  }

  static factory(stub: StubWorker): typeof Worker {
    return function MockWorker() {
      return stub;
    } as unknown as typeof Worker;
  }
}
