import { afterEach, describe, expect, it, vi } from "vitest";
import { DegradedRuntime, WasmBridge, eventCorrelationId } from "@nmp/runtime-web";
import type { WorkerEvent, WorkerRequest } from "@nmp/runtime-web";
import { createNmpClient } from "./client";

const routingJson = "{\"schema_version\":1,\"capacity\":64,\"publishes\":[],\"subscriptions\":[]}";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe("routing diagnostics runtime flow", () => {
  it("degrades routing diagnostics as capability failure when wasm is unavailable", () => {
    const runtime = new DegradedRuntime();

    expect(runtime.handle({ type: "routing_decisions", correlation_id: "routing-1" })).toEqual([
      {
        type: "capability_failure",
        capability: "nmp.routing_decisions",
        correlation_id: "routing-1",
        reason: "nmp-wasm actor driver is not linked into the web worker yet",
      },
    ]);
  });

  it("correlates routing diagnostics worker responses", () => {
    expect(
      eventCorrelationId({
        type: "routing_decisions",
        correlation_id: "routing-1",
        json: "{}",
      }),
    ).toBe("routing-1");
  });

  it("stores routing decisions returned by the worker diagnostics request", async () => {
    const worker = new StubWorker();
    const posted: WorkerRequest[] = [];
    worker.postMessage = (request: WorkerRequest) => {
      posted.push(request);
      if (request.type === "hello") {
        worker.emit({ type: "hello_accepted", protocol_version: 1, status: "ready" });
      }
      if (request.type === "routing_decisions") {
        worker.emit({
          type: "routing_decisions",
          correlation_id: request.correlation_id,
          json: routingJson,
        });
      }
    };
    vi.stubGlobal("Worker", StubWorker.factory(worker));

    const client = createNmpClient();
    const snapshot = await client.refreshRoutingDecisions();

    expect(posted.some((request) => request.type === "routing_decisions")).toBe(true);
    expect(snapshot.latestRoutingDecisionsJson).toContain("\"schema_version\":1");
  });

  it("wraps the wasm routing diagnostics accessor as a worker event", () => {
    const bridge = new WasmBridge(
      {
        handle_json: () => [],
        recent_routing_decisions: () => routingJson,
      },
      () => {},
      () => {},
    );

    expect(bridge.handle({ type: "routing_decisions", correlation_id: "routing-2" })).toEqual([
      {
        type: "routing_decisions",
        correlation_id: "routing-2",
        json: routingJson,
      },
    ]);
  });
});

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
