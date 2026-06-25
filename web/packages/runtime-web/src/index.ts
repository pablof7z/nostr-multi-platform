// Protocol types and helpers
export type {
  WorkerRequest,
  WorkerEvent,
  WorkerEventSummary,
  RuntimeStatus,
  ChirpAction,
} from "./protocol";
export { protocolVersion, eventCorrelationId, labelRuntimeStatus, summarizeWorkerEvent } from "./protocol";

// ADR-0064 typed write transport — the `DispatchEnvelope` byte encoder.
export {
  encodeDispatchEnvelope,
  DISPATCH_ENVELOPE_FILE_IDENTIFIER,
  DISPATCH_ENVELOPE_SCHEMA_VERSION,
} from "./dispatchEnvelope";

// ADR-0064 §3 (#1776) — generated typed write builders. Field-level sugar that
// encodes the per-crate FlatBuffers payload + wraps it in a `DispatchEnvelope`
// for the `dispatch_bytes` doorway, so the host never hand-assembles FlatBuffers
// or spells an `action_namespace`. GENERATED — see
// `crates/nmp-codegen/src/action_builders/registry.rs`.
export { GeneratedActionBuilders } from "./actionBuilders.generated";

// Wasm bridge
export type { WasmBridgeLoadResult, WasmBridgeUnavailable, UpdateBytesSink } from "./wasmBridge";
export { WasmBridge, loadWasmBridge } from "./wasmBridge";

// Degraded runtime
export type { DegradedRuntimeMode } from "./degradedRuntime";
export { DegradedRuntime } from "./degradedRuntime";

// Npub encoding utility (calls Rust NIP-19 encoder — never browser bech32)
export { encodeNpub } from "./encodeNpub";
