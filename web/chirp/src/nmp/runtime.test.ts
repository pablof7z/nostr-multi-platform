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
import { FrameKind, PanicFrame, RelayStatus, SnapshotFrame, UpdateFrame } from "./generated/nmp/transport";
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

  it("decodes Tier-3 running and relayStatuses from a snapshot frame", () => {
    const bytes = makeSnapshotBytes({
      running: true,
      relayStatuses: [{ url: "wss://relay.example", role: "both", connection: "Connected" }],
    });
    const decoded = decodeUpdateFrameBytes(bytes);

    // Use objectContaining so the test is resilient to new fields added to
    // DecodedUpdateFrame/DecodedRelayStatus without being brittle.
    expect(decoded).toMatchObject({
      type: "snapshot",
      schemaVersion: 1,
      running: true,
      relayStatuses: [
        expect.objectContaining({ url: "wss://relay.example", role: "both", status: "Connected" }),
      ],
    });
  });

  it("decodes a snapshot frame with no relay rows", () => {
    const bytes = makeSnapshotBytes({ running: false });
    const decoded = decodeUpdateFrameBytes(bytes);

    expect(decoded).toMatchObject({
      type: "snapshot",
      schemaVersion: 1,
      running: false,
      relayStatuses: [],
    });
  });

  it("matches the Rust golden snapshot v1 fixture (pre-Tier3: relay_statuses absent, defaults)", () => {
    // The v1 golden fixture was built before Tier-3 fields were added (PR-B).
    // FlatBuffers vtable lookup returns defaults for absent fields, so decoding
    // this old wire format must succeed and report zero relay rows.
    const hex = goldenSnapshotV1Hex.replace(/\s+/g, "");
    if (hex.length % 2 !== 0) {
      throw new Error("hex fixture must contain full bytes");
    }
    const bytes = new Uint8Array(hex.length / 2);
    for (let i = 0; i < bytes.length; i += 1) {
      bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
    }

    const decoded = decodeUpdateFrameBytes(bytes);
    expect(decoded).toMatchObject({
      type: "snapshot",
      schemaVersion: 1,
      running: false, // Tier-3 `running` field absent in v1 fixture → default false
      relayStatuses: [], // Tier-3 `relay_statuses` absent in v1 fixture → empty
    });
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
          configured_relays: [{ url: "wss://relay.example", role: "both" }],
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
  it("degrades to protocol_mismatch when the envelope schema version diverges", async () => {
    const stub = new StubWorker();
    vi.stubGlobal("Worker", StubWorker.factory(stub));

    const client = createNmpClient();
    const snapshots: RuntimeSnapshot[] = [];
    const unsubscribe = client.subscribe((snapshot) => snapshots.push(snapshot));

    stub.emit({
      type: "update_bytes",
      bytes: makeSnapshotBytes({ running: false }, 2),
    });

    unsubscribe();
    expect(snapshots[snapshots.length - 1]!.status).toEqual({ degraded: "protocol_mismatch" });
  });

  it("advances status to running when the Tier-3 running field is true", async () => {
    const stub = new StubWorker();
    vi.stubGlobal("Worker", StubWorker.factory(stub));

    const client = createNmpClient();
    const snapshots: RuntimeSnapshot[] = [];
    const unsubscribe = client.subscribe((snapshot) => snapshots.push(snapshot));

    stub.emit({
      type: "update_bytes",
      bytes: makeSnapshotBytes({ running: true }),
    });

    unsubscribe();
    expect(snapshots[snapshots.length - 1]!.status).toBe("running");
  });

  it("populates latestRelayStatuses from the Tier-3 relay_statuses field", async () => {
    const stub = new StubWorker();
    vi.stubGlobal("Worker", StubWorker.factory(stub));

    const client = createNmpClient();
    const snapshots: RuntimeSnapshot[] = [];
    const unsubscribe = client.subscribe((snapshot) => snapshots.push(snapshot));

    stub.emit({
      type: "update_bytes",
      bytes: makeSnapshotBytes({
        running: true,
        relayStatuses: [{ url: "wss://r.example", role: "both", connection: "Connected" }],
      }),
    });

    unsubscribe();
    expect(snapshots[snapshots.length - 1]!.latestRelayStatuses).toEqual([
      expect.objectContaining({ url: "wss://r.example", role: "both", status: "Connected" }),
    ]);
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

type SnapshotBytesOptions = {
  running?: boolean;
  relayStatuses?: Array<{ url: string; role: string; connection: string }>;
};

/** Build a minimal UpdateFrame binary for unit tests using Tier-3 fields.
 *  The deprecated `payload:Value` field (PR-B #991/#979) is intentionally
 *  omitted — the kernel zeroes it and the TS decoder no longer reads it. */
function makeSnapshotBytes(
  options: SnapshotBytesOptions = {},
  schemaVersion = 1,
): Uint8Array {
  const builder = new flatbuffers.Builder(256);

  // Build relay status offsets first (nested objects must precede parent).
  let relayStatusesVecOffset: flatbuffers.Offset | undefined;
  if (options.relayStatuses && options.relayStatuses.length > 0) {
    const offsets = options.relayStatuses.map(({ url, role, connection }) => {
      const urlOff = builder.createString(url);
      const roleOff = builder.createString(role);
      const connOff = builder.createString(connection);
      return RelayStatus.createRelayStatus(
        builder,
        roleOff,
        urlOff,
        connOff,
        0, 0, BigInt(0), 0, null, null, 0, 0, 0, BigInt(0), BigInt(0), BigInt(0), false, 0,
      );
    });
    relayStatusesVecOffset = SnapshotFrame.createRelayStatusesVector(builder, offsets);
  }

  SnapshotFrame.startSnapshotFrame(builder);
  SnapshotFrame.addSchemaVersion(builder, schemaVersion);
  if (options.running !== undefined) {
    SnapshotFrame.addRunning(builder, options.running);
  }
  if (relayStatusesVecOffset !== undefined) {
    SnapshotFrame.addRelayStatuses(builder, relayStatusesVecOffset);
  }
  const snapshotOffset = SnapshotFrame.endSnapshotFrame(builder);

  UpdateFrame.startUpdateFrame(builder);
  UpdateFrame.addKind(builder, FrameKind.Snapshot);
  UpdateFrame.addSnapshot(builder, snapshotOffset);
  const frameOffset = UpdateFrame.endUpdateFrame(builder);
  UpdateFrame.finishUpdateFrameBuffer(builder, frameOffset);
  return builder.asUint8Array();
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
