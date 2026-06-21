// @nmp/runtime-web — shared NMP web runtime package.
// Owns: worker protocol types, WASM bridge, degraded runtime, pure utilities.
// Apps import from here; do NOT duplicate these files per app.
export type {
  WorkerRequest,
  WorkerEvent,
  RuntimeStatus,
} from "./protocol";
export {
  protocolVersion,
  eventCorrelationId,
  labelRuntimeStatus,
} from "./protocol";
export type {
  WasmBridgeLoadResult,
  WasmBridgeUnavailable,
  UpdateBytesSink,
} from "./wasmBridge";
export { WasmBridge, loadWasmBridge } from "./wasmBridge";
export type { DegradedRuntimeMode } from "./degradedRuntime";
export { DegradedRuntime } from "./degradedRuntime";
export { startNmpWorker } from "./worker-init";
export type { NpubResult } from "./utils/npub";
export { encodeNpub } from "./utils/npub";
