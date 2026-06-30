import type { WorkerEvent, WorkerRequest } from "./protocol";

// Package-relative module URL for the wasm composition root emitted by
// wasm-pack (`nmp_browser_runtime.js`, underscore form from the crate name).
const defaultModuleUrl = new URL("./wasm/nmp_browser_runtime.js", import.meta.url).toString();

type SnapshotCallback = (bytes: Uint8Array) => void;

type NmpWasmRuntime = {
  handle_json(request: string): unknown;
  /** Binary write doorway (#1008 / ADR-0064): receives the raw `Uint8Array` of
   *  a `DispatchEnvelope` FlatBuffers root directly, bypassing the
   *  `JSON.stringify(Uint8Array) → {}` corruption that occurs on `handle_json`.
   *  Added alongside `handle_json` so the bridge can detect whether the loaded
   *  module supports the binary path and fall back gracefully. */
  handle_dispatch_bytes?(bytes: Uint8Array): unknown;
  recent_routing_decisions?(): string;
  set_snapshot_callback?(callback: SnapshotCallback | null): void;
  /** Async pre-`Start` hook (#1007): open the durable OPFS-SQLite store and park
   *  it on the runtime so the subsequent synchronous `Start` injects it instead
   *  of an in-memory store. The host MUST `await` this BEFORE dispatching the
   *  `Start` request. On open failure the runtime falls back to in-memory and
   *  records a Tier-3 `store_open_failure` diagnostic (PR-8) — it never throws.
   *  Absent on a wasm build compiled without the `opfs-sqlite-backend` feature
   *  (the runtime then simply starts in-memory). */
  prepare_store?(appId: string, databaseName: string): Promise<void>;
};

/** Free function exported by the wasm module: hex pubkey → JSON `{npub, npubShort}`
 *  (canonical Rust NIP-19 encoder), or undefined/null on invalid input. */
type EncodeNpubFn = (hex: string) => string | undefined | null;

type NmpWasmModule = {
  default?: (input?: unknown) => Promise<unknown> | unknown;
  NmpWasmRuntime?: new () => NmpWasmRuntime;
  nmp_encode_npub?: EncodeNpubFn;
};

export type WasmBridgeUnavailable = {
  code: "wasm_bridge_unavailable";
  message: string;
};

export type WasmBridgeLoadResult =
  | { type: "loaded"; bridge: WasmBridge }
  | { type: "unavailable"; error: WasmBridgeUnavailable };

/// The wasm runtime delivers FlatBuffers update bytes through a registered
/// JS callback rather than through `handle_json`'s return value. Encoding
/// ~870KB of binary frame as a JSON number array on every 4Hz tick defeats
/// the binary transport; the typed-array sink keeps the bytes binary.
///
/// The sink is installed once at bridge construction time and stays
/// installed for the bridge's lifetime — the wasm runtime calls it
/// synchronously from inside `handle_json` (for `Start`/dispatch-driven
/// snapshots) and from the relay-pool sink (for inbound-driven snapshots).
export type UpdateBytesSink = (bytes: Uint8Array) => void;

export class WasmBridge {
  constructor(
    private readonly runtime: NmpWasmRuntime,
    private readonly onUpdateBytes: UpdateBytesSink,
    private readonly encodeNpubFn?: EncodeNpubFn,
  ) {
    runtime.set_snapshot_callback?.((bytes) => {
      // The runtime hands us a fresh `Uint8Array` owned by the JS heap
      // (see `push_bytes_if_callback` in snapshot.rs — `copy_from` allocs
      // into JS memory). Forwarding the same instance is safe; the sink
      // is responsible for any further copy/transfer semantics.
      this.onUpdateBytes(bytes);
    });
  }

  /** Encode a hex pubkey via the Rust NIP-19 encoder. Returns `{}` when the
   *  encoder is absent or the pubkey is invalid (D6 — honest empty, no throw). */
  encodeNpub(pubkey: string): { npub?: string; npubShort?: string } {
    const json = this.encodeNpubFn?.(pubkey);
    if (!json) return {};
    try {
      const parsed = JSON.parse(json) as { npub?: string; npubShort?: string };
      return { npub: parsed.npub, npubShort: parsed.npubShort };
    } catch {
      return {};
    }
  }

  /** Async pre-`Start` durable-store open (#1007). The worker MUST `await` this
   *  before calling {@link handle} so a `Start` request injects the OPFS-SQLite
   *  store rather than starting in-memory. For non-`start` requests this is a
   *  no-op; for `start` it opens the per-app OPFS namespace
   *  (`app_id` + `database_name`). The wasm `prepare_store` never rejects — on an
   *  OPFS open failure it falls back to in-memory and records the degraded-mode
   *  `store_open_failure` diagnostic (PR-8) — but we still guard defensively so a
   *  thrown value can never abort the worker before `Start` is dispatched. */
  async prepareForStart(request: WorkerRequest): Promise<void> {
    if (request.type !== "start") return;
    if (typeof this.runtime.prepare_store !== "function") return;
    try {
      await this.runtime.prepare_store(request.app_id, request.database_name);
    } catch (error) {
      // Honest degrade: the runtime starts in-memory and the failure surfaces
      // through the snapshot's store_open_failure (PR-8). Don't block Start.
      console.error("[NMP] prepare_store failed; starting in-memory", error);
    }
  }

  handle(request: WorkerRequest): WorkerEvent[] {
    try {
      // #1008 / ADR-0064 — binary write doorway: route `dispatch_bytes`
      // through `handle_dispatch_bytes` (if available) to avoid the
      // `JSON.stringify(Uint8Array) → {}` corruption that zeros the bytes
      // on the generic `handle_json` path. The `bytes` field is a `Uint8Array`
      // from the structured-clone message; `JSON.stringify` cannot round-trip
      // typed arrays, so only the direct binary path preserves the payload.
      if (request.type === "dispatch_bytes") {
        if (typeof this.runtime.handle_dispatch_bytes === "function") {
          return decodeWorkerEvents(this.runtime.handle_dispatch_bytes(request.bytes));
        } else {
          // fail-closed: no JSON fallback for typed writes — JSON.stringify
          // cannot round-trip Uint8Array (serialises to `{}`), so falling
          // through to handle_json would silently corrupt the payload.
          console.error("[NMP] handle_dispatch_bytes not available — bridge not initialized");
          throw new Error("dispatch_bytes called before bridge initialization");
        }
      }
      if (request.type === "routing_decisions") {
        if (typeof this.runtime.recent_routing_decisions !== "function") {
          return [
            {
              type: "error",
              code: "routing_decisions_unavailable",
              message: "nmp-browser-runtime module loaded without recent_routing_decisions export",
              correlation_id: request.correlation_id,
            },
          ];
        }
        return [
          {
            type: "routing_decisions",
            correlation_id: request.correlation_id,
            json: this.runtime.recent_routing_decisions(),
          },
        ];
      }
      return decodeWorkerEvents(this.runtime.handle_json(JSON.stringify(request)));
    } catch (error) {
      return [
        {
          type: "error",
          code: "wasm_runtime_error",
          message: messageFrom(error, "browser runtime failed"),
          correlation_id: requestCorrelationId(request),
        },
      ];
    }
  }
}

export async function loadWasmBridge(
  onUpdateBytes: UpdateBytesSink,
  modulePath: string | URL = defaultModuleUrl,
): Promise<WasmBridgeLoadResult> {
  const moduleUrl = resolveWasmModuleUrl(modulePath);
  try {
    if (!(await moduleAssetAvailable(moduleUrl))) {
      return unavailable(`nmp-browser-runtime module is not available at ${moduleUrl}`);
    }
    const wasmModule = (await import(/* @vite-ignore */ moduleUrl)) as NmpWasmModule;
    if (typeof wasmModule.default === "function") {
      await wasmModule.default();
    }
    if (typeof wasmModule.NmpWasmRuntime !== "function") {
      return unavailable("nmp-browser-runtime module loaded without NmpWasmRuntime export");
    }
    return {
      type: "loaded",
      bridge: new WasmBridge(new wasmModule.NmpWasmRuntime(), onUpdateBytes, wasmModule.nmp_encode_npub),
    };
  } catch (error) {
    return unavailable(`nmp-browser-runtime module could not be loaded from ${moduleUrl}`);
  }
}

function resolveWasmModuleUrl(modulePath: string | URL): string {
  return new URL(modulePath.toString(), import.meta.url).toString();
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

function decodeWorkerEvents(value: unknown): WorkerEvent[] {
  const event = typeof value === "string" ? (JSON.parse(value) as unknown) : value;
  const events = Array.isArray(event) ? event : [event];
  for (const item of events) {
    if (!isWorkerEvent(item)) {
      throw new Error("browser runtime returned an invalid worker event");
    }
  }
  return events;
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
