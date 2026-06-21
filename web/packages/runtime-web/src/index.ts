// Protocol types and helpers
export type {
  WorkerRequest,
  WorkerEvent,
  RuntimeStatus,
  ChirpAction,
} from "./protocol";
export { protocolVersion, eventCorrelationId, labelRuntimeStatus } from "./protocol";

// Wasm bridge
export type { WasmBridgeLoadResult, WasmBridgeUnavailable, UpdateBytesSink } from "./wasmBridge";
export { WasmBridge, loadWasmBridge } from "./wasmBridge";

// Degraded runtime
export type { DegradedRuntimeMode } from "./degradedRuntime";
export { DegradedRuntime } from "./degradedRuntime";

// Npub encoding utility (calls Rust NIP-19 encoder — never browser bech32)
export { encodeNpub } from "./encodeNpub";
