import { afterEach, describe, expect, it, vi } from "vitest";
import { publishNoteAction } from "./actions";
import type { WorkerEvent, WorkerRequest } from "@nmp/runtime-web";
import { protocolVersion } from "@nmp/runtime-web";

type WorkerHarness = {
  onmessage: ((message: MessageEvent<WorkerRequest>) => void) | null;
  postMessage: (event: WorkerEvent) => void;
};

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
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

    await import("@nmp/runtime-web/worker");

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
      relay_bootstrap: [{ url: "wss://relay.example", role: "both,indexer" }],
      database_name: "chirp-test",
      correlation_id: "start-1",
    });
    await sendWorkerRequest(harness, {
      type: "app_action",
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
        capability: "app_action",
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
