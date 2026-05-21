import { afterEach, describe, expect, it, vi } from "vitest";
import { publishNoteAction } from "./actions";
import { createNmpClient } from "./client";
import { DegradedRuntime } from "./degradedRuntime";
import type { WorkerEvent, WorkerRequest } from "./protocol";
import { protocolVersion } from "./protocol";
import { chirpTimelineFromEnvelope, displayRows, kernelSnapshotFromEnvelope } from "./snapshot";

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
        relays: ["wss://relay.example"],
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
        action_type: "chirp.compose",
        payload: { content: "hello" },
        correlation_id: "dispatch-1",
      }),
    ).toEqual([
      {
        type: "capability_failure",
        capability: "chirp.compose",
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
    const dispatched = await client.dispatch("chirp.compose", { content: "hello" });
    unsubscribe();

    expect(snapshots[0].events[0]).toMatchObject({ type: "hello_accepted" });
    expect(started.status).toEqual({ degraded: "browser_bridge_unavailable" });
    expect(started.events[0]).toMatchObject({
      type: "runtime_status",
      correlation_id: "web-start",
    });
    expect(dispatched.events[0]).toMatchObject({
      type: "capability_failure",
      capability: "chirp.compose",
      reason: "Web Worker support is unavailable, so the nmp-wasm bridge cannot start",
    });
  });
});

describe("shared Chirp web semantics", () => {
  it("builds the same publish note action body used by native and chirp-repl", () => {
    expect(publishNoteAction("hello web")).toEqual({
      PublishNote: {
        content: "hello web",
        reply_to_id: null,
        target: "Auto",
      },
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
      relays: ["wss://relay.example"],
      database_name: "chirp-test",
      correlation_id: "start-1",
    });
    await sendWorkerRequest(harness, {
      type: "dispatch",
      action_type: "chirp.compose",
      payload: { content: "hello" },
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
        capability: "chirp.compose",
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
