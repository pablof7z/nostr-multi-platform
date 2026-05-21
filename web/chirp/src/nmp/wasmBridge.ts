import type { WorkerEvent, WorkerRequest } from "./protocol";

const defaultModulePath = "/nmp-wasm/nmp_wasm.js";

type NmpWasmRuntime = {
  handle_json(request: string): unknown;
};

type NmpWasmModule = {
  default?: (input?: unknown) => Promise<unknown> | unknown;
  NmpWasmRuntime?: new () => NmpWasmRuntime;
};

export type WasmBridgeUnavailable = {
  code: "wasm_bridge_unavailable";
  message: string;
};

export type WasmBridgeLoadResult =
  | { type: "loaded"; bridge: WasmBridge }
  | { type: "unavailable"; error: WasmBridgeUnavailable };

export class WasmBridge {
  constructor(private readonly runtime: NmpWasmRuntime) {}

  handle(request: WorkerRequest): WorkerEvent[] {
    try {
      return [decodeWorkerEvent(this.runtime.handle_json(JSON.stringify(request)))];
    } catch (error) {
      return [
        {
          type: "error",
          code: "wasm_runtime_error",
          message: messageFrom(error, "nmp-wasm runtime failed"),
          correlation_id: requestCorrelationId(request),
        },
      ];
    }
  }
}

export async function loadWasmBridge(
  modulePath = defaultModulePath,
): Promise<WasmBridgeLoadResult> {
  try {
    const moduleUrl = new URL(modulePath, workerOrigin()).toString();
    if (!(await moduleAssetAvailable(moduleUrl))) {
      return unavailable(`nmp-wasm module is not available at ${modulePath}`);
    }
    const wasmModule = (await import(/* @vite-ignore */ moduleUrl)) as NmpWasmModule;
    if (typeof wasmModule.default === "function") {
      await wasmModule.default();
    }
    if (typeof wasmModule.NmpWasmRuntime !== "function") {
      return unavailable("nmp-wasm module loaded without NmpWasmRuntime export");
    }
    return { type: "loaded", bridge: new WasmBridge(new wasmModule.NmpWasmRuntime()) };
  } catch (error) {
    return unavailable(`nmp-wasm module could not be loaded from ${modulePath}`);
  }
}

async function moduleAssetAvailable(moduleUrl: string): Promise<boolean> {
  const workerSelf =
    typeof self === "undefined" ? undefined : (self as unknown as { fetch?: typeof fetch });
  const fetcher = workerSelf?.fetch ?? globalThis.fetch;
  if (typeof fetcher !== "function") {
    return true;
  }
  try {
    const response = await fetcher(moduleUrl, { method: "HEAD", cache: "no-store" });
    if (!response.ok) {
      return false;
    }
    return isJavaScriptModule(response.headers.get("content-type") ?? "");
  } catch {
    return false;
  }
}

function isJavaScriptModule(contentType: string): boolean {
  const normalized = contentType.toLowerCase();
  return (
    normalized.length === 0 ||
    normalized.includes("javascript") ||
    normalized.includes("ecmascript")
  );
}

function decodeWorkerEvent(value: unknown): WorkerEvent {
  const event = typeof value === "string" ? (JSON.parse(value) as unknown) : value;
  if (!isWorkerEvent(event)) {
    throw new Error("nmp-wasm returned an invalid worker event");
  }
  return event;
}

function isWorkerEvent(event: unknown): event is WorkerEvent {
  return (
    typeof event === "object" &&
    event !== null &&
    "type" in event &&
    typeof (event as { type: unknown }).type === "string"
  );
}

function requestCorrelationId(request: WorkerRequest): string | undefined {
  return "correlation_id" in request ? request.correlation_id : undefined;
}

function workerOrigin(): string {
  const location = (self as unknown as { location?: { origin?: string } }).location;
  return location?.origin ?? "http://localhost";
}

function unavailable(message: string): WasmBridgeLoadResult {
  return {
    type: "unavailable",
    error: {
      code: "wasm_bridge_unavailable",
      message,
    },
  };
}

function messageFrom(error: unknown, fallback: string): string {
  return error instanceof Error && error.message.length > 0 ? error.message : fallback;
}
