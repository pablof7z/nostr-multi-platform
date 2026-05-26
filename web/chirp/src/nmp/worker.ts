import { DegradedRuntime } from "./degradedRuntime";
import type { WorkerEvent, WorkerRequest } from "./protocol";
import { loadWasmBridge, type WasmBridge } from "./wasmBridge";

const scope = self as unknown as {
  onmessage: ((message: MessageEvent<WorkerRequest>) => void) | null;
  postMessage: (message: WorkerEvent) => void;
};

type Runtime = DegradedRuntime | WasmBridge;

// Snapshot bytes arrive on a side channel — the wasm bridge invokes this
// sink synchronously from inside `handle_json` (Start/dispatch snapshot)
// and from the relay-pool sink (inbound-driven snapshot). Re-emit as a
// normal `update_bytes` event so the main thread keeps a single event
// channel. No transfer list — `client.ts` caches the bytes across snapshots
// and a transferred ArrayBuffer detaches, breaking the cache; structured
// clone copies the small (~870KB at worst) frame cheaply.
const emitUpdateBytes = (bytes: Uint8Array) => {
  scope.postMessage({ type: "update_bytes", bytes });
};

const runtime = initializeRuntime();
let startupEventsSent = false;

scope.onmessage = async (message: MessageEvent<WorkerRequest>) => {
  try {
    const initialized = await runtime;
    if (!startupEventsSent) {
      startupEventsSent = true;
      for (const event of initialized.startupEvents) {
        scope.postMessage(event);
      }
    }
    for (const event of initialized.runtime.handle(message.data)) {
      scope.postMessage(event);
    }
  } catch (error) {
    const event: WorkerEvent = {
      type: "error",
      code: "worker_exception",
      message: error instanceof Error ? error.message : "worker failed",
    };
    scope.postMessage(event);
  }
};

async function initializeRuntime(): Promise<{
  runtime: Runtime;
  startupEvents: WorkerEvent[];
}> {
  const loaded = await loadWasmBridge(emitUpdateBytes);
  if (loaded.type === "loaded") {
    return { runtime: loaded.bridge, startupEvents: [] };
  }
  return {
    runtime: new DegradedRuntime("browser_bridge_unavailable", loaded.error.message),
    startupEvents: [
      {
        type: "error",
        code: loaded.error.code,
        message: loaded.error.message,
      },
    ],
  };
}
