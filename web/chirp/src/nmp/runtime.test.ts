import { afterEach, describe, expect, it, vi } from "vitest";
import {
  discoverGroupsCommand,
  publishNoteAction,
  reactGroupMessageCommand,
  replyGroupMessageCommand,
  sendDmCommand,
  walletCommand,
} from "./actions";
import { createNmpClient, makeCorrelationId, type RuntimeSnapshot } from "./client";
import { DegradedRuntime } from "@nmp/runtime-web";
import * as flatbuffers from "flatbuffers";
import type { WorkerEvent, WorkerRequest } from "@nmp/runtime-web";
import { eventCorrelationId, protocolVersion } from "@nmp/runtime-web";
import { chirpTimelineFromEnvelope, featureSnapshotFromEnvelope, kernelSnapshotFromEnvelope } from "./snapshot";
import { FrameKind, PanicFrame, RelayStatus, SnapshotFrame, UpdateFrame } from "./generated/nmp/transport";
import { decodeUpdateFrameBytes, UpdateFrameDecodeError } from "./updateFrame";

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
        relays: ["wss://relay.example"],
        relay_bootstrap: [{ url: "wss://relay.example", role: "both,indexer" }],
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

describe("makeCorrelationId", () => {
  it("produces distinct ids for consecutive sequence numbers (same-tick dispatch safety)", () => {
    // Two dispatches fired in the same millisecond used to collide because
    // Date.now() can return the same value.  The monotonic seq counter makes
    // each id unique regardless of clock resolution.
    const id0 = makeCorrelationId("web", 0);
    const id1 = makeCorrelationId("web", 1);
    expect(id0).not.toBe(id1);
  });

  it("encodes the prefix so signer ids cannot collide with dispatch ids", () => {
    // Both use seq=0 but distinct prefixes must yield distinct strings.
    const dispatch = makeCorrelationId("web", 0);
    const signer = makeCorrelationId("web-signer", 0);
    expect(dispatch).not.toBe(signer);
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
      capability: "app_action",
      reason: "Web Worker support is unavailable, so the nmp-wasm bridge cannot start",
    });
  });
});

describe("shared Chirp web semantics", () => {
  it("treats binary update events as out-of-band snapshot transport", () => {
    expect(eventCorrelationId({ type: "update_bytes", bytes: new Uint8Array([1, 2, 3]) })).toBeUndefined();
  });

  // #1753 S6 — sign round-trip terminals are correlation-keyed so a pending
  // begin resolves on completion; `sign_request` is a broker instruction (not a
  // reply) so it carries no resolvable correlation id.
  it("routes sign round-trip terminals by correlation id but not sign_request", () => {
    expect(
      eventCorrelationId({ type: "sign_completed", correlation_id: "c1", signed_json: "{}" }),
    ).toBe("c1");
    expect(eventCorrelationId({ type: "sign_failed", correlation_id: "c2", reason: "x" })).toBe("c2");
    expect(
      eventCorrelationId({
        type: "sign_request",
        correlation_id: "c3",
        account_pubkey: "ab",
        unsigned_json: "{}",
      }),
    ).toBeUndefined();
  });

  it("degraded beginSign fails the round-trip closed (no kernel to park in)", () => {
    vi.stubGlobal("Worker", undefined);
    const client = createNmpClient();
    client.beginSign(
      "ab".repeat(32),
      JSON.stringify({ pubkey: "ab", kind: 1, tags: [], content: "x", created_at: 0 }),
    );
    const snap = client.snapshot();
    expect(snap.events.some((e) => e.type === "sign_failed")).toBe(true);
  });

  it("decodes Tier-3 running and relayStatuses from a snapshot frame", () => {
    const bytes = makeSnapshotBytes({
      running: true,
      relayStatuses: [{ url: "wss://relay.example", role: "both", connection: "Connected" }],
    });
    const decoded = decodeUpdateFrameBytes(bytes);

    expect(decoded).toMatchObject({
      type: "snapshot",
      schemaVersion: 1,
      running: true,
      relayStatuses: [expect.objectContaining({ url: "wss://relay.example", role: "both", status: "Connected" })],
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

  it("parses rev from a Rust snapshot envelope (generic projections map deleted)", () => {
    // PR #1515 removed the generic JSON `projections` map from KernelSnapshot.
    // `kernelSnapshotFromEnvelope` still extracts `rev`; any `projections` key
    // in the payload is silently ignored (not a type error, just extra data).
    const kernel = kernelSnapshotFromEnvelope({
      t: "snapshot",
      v: { rev: 7 },
    });
    expect(kernel?.rev).toBe(7);
  });

  it("parses Chirp modular snapshot card shape via chirpTimelineFromEnvelope", () => {
    // `displayRows` was deleted alongside `timelineFromKernel` (PR #1515).
    // Callers that need Chirp cards use `chirpTimelineFromEnvelope` directly.
    const chirp = chirpTimelineFromEnvelope({
      chirpTimeline: {
        blocks: [{ Standalone: "note-a" }],
        cards: [{ id: "note-a", author_pubkey: "abc", content: "from chirp cards" }],
      },
    });

    expect(chirp?.cards[0]).toMatchObject({ id: "note-a", author_pubkey: "abc" });
  });

  it("featureSnapshotFromEnvelope returns the canonical zero-state constant (generic projections lane deleted)", () => {
    // The generic JSON `projections` map was deleted in PR #1515 (escape hatch
    // #2 eliminated). `featureSnapshotFromEnvelope` is now a constant-return
    // with no dead helpers (dmFrom, groupsFrom, messagesFrom, projection).
    // This test proves: (a) the function returns zero-state for any input, and
    // (b) the same reference is returned regardless of the envelope value —
    // proving no parsing happens inside.
    const featureWithEnvelope = featureSnapshotFromEnvelope({ t: "snapshot", v: { rev: 1 } });
    const featureWithUndefined = featureSnapshotFromEnvelope(undefined);
    const featureWithNull = featureSnapshotFromEnvelope(null);

    // All calls return identical state (same constant object).
    expect(featureWithEnvelope).toBe(featureWithUndefined);
    expect(featureWithEnvelope).toBe(featureWithNull);

    // All fields are zero-state.
    expect(featureWithEnvelope.accounts).toEqual([]);
    expect(featureWithEnvelope.dmConversations).toEqual([]);
    expect(featureWithEnvelope.discoveredGroups).toEqual([]);
    expect(featureWithEnvelope.groupMessages).toEqual([]);
    expect(featureWithEnvelope.relayDiagnostics).toEqual([]);
    expect(featureWithEnvelope.outbox).toEqual([]);
    expect(featureWithEnvelope.followCount).toBe(0);
    expect(featureWithEnvelope.activeAccount).toBe("");
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
