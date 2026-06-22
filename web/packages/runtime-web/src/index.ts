// Protocol types and helpers
export type {
  WorkerRequest,
  WorkerEvent,
  RuntimeStatus,
  ChirpAction,
} from "./protocol";
export { protocolVersion, eventCorrelationId, labelRuntimeStatus } from "./protocol";

// ADR-0064 typed write transport — the `DispatchEnvelope` byte encoder.
export {
  encodeDispatchEnvelope,
  DISPATCH_ENVELOPE_FILE_IDENTIFIER,
  DISPATCH_ENVELOPE_SCHEMA_VERSION,
} from "./dispatchEnvelope";

// Wasm bridge
export type { WasmBridgeLoadResult, WasmBridgeUnavailable, UpdateBytesSink } from "./wasmBridge";
export { WasmBridge, loadWasmBridge } from "./wasmBridge";

// Degraded runtime
export type { DegradedRuntimeMode } from "./degradedRuntime";
export { DegradedRuntime } from "./degradedRuntime";

// Npub encoding utility (calls Rust NIP-19 encoder — never browser bech32)
export { encodeNpub } from "./encodeNpub";
