import { afterEach, describe, expect, it, vi } from "vitest";
import { createNmpClient } from "./client";
import { DegradedRuntime } from "./degradedRuntime";
import type { WorkerEvent, WorkerRequest } from "./protocol";
import { protocolVersion } from "./protocol";

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
    expect(started.status).toEqual({ degraded: "browser_actor_driver_missing" });
    expect(started.events[0]).toMatchObject({
      type: "runtime_status",
      correlation_id: "web-start",
    });
    expect(dispatched.events[0]).toMatchObject({
      type: "capability_failure",
      capability: "chirp.compose",
      reason: "nmp-wasm actor driver is not linked into the web worker yet",
    });
  });
});

describe("worker runtime bridge", () => {
  it("posts hello/start/dispatch events from worker requests", async () => {
    const events: WorkerEvent[] = [];
    const harness: WorkerHarness = {
      onmessage: null,
      postMessage: (event) => events.push(event),
    };
    vi.stubGlobal("self", harness);

    await import("./worker");

    sendWorkerRequest(harness, {
      type: "hello",
      app_id: "chirp",
      platform: "web",
      protocol_version: protocolVersion,
    });
    sendWorkerRequest(harness, {
      type: "start",
      app_id: "chirp",
      relays: ["wss://relay.example"],
      database_name: "chirp-test",
      correlation_id: "start-1",
    });
    sendWorkerRequest(harness, {
      type: "dispatch",
      action_type: "chirp.compose",
      payload: { content: "hello" },
      correlation_id: "dispatch-1",
    });

    expect(events).toEqual([
      { type: "hello_accepted", protocol_version: 1, status: "ready" },
      {
        type: "runtime_status",
        status: { degraded: "browser_actor_driver_missing" },
        correlation_id: "start-1",
      },
      {
        type: "capability_failure",
        capability: "chirp.compose",
        correlation_id: "dispatch-1",
        reason: "nmp-wasm actor driver is not linked into the web worker yet",
      },
    ]);
  });
});

function sendWorkerRequest(harness: WorkerHarness, request: WorkerRequest) {
  if (!harness.onmessage) {
    throw new Error("worker did not register an onmessage handler");
  }
  harness.onmessage({ data: request } as MessageEvent<WorkerRequest>);
}
