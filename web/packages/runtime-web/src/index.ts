// Protocol types and helpers
export type {
  WorkerRequest,
  WorkerEvent,
  RuntimeStatus,
  IdentityRelayPermission,
  FeedSessionHandle,
} from "./protocol";
export { protocolVersion, eventCorrelationId, labelRuntimeStatus } from "./protocol";

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

// #1626 generated typed feed helpers. Field-level sugar that builds canonical
// FeedParams JSON and targets the `feed_open_json` Worker control; Rust owns the
// session compiler, source graph, and teardown.
export type { FeedHelperShape, FeedRuntime } from "./feedHelpers.generated";
export { GeneratedFeedHelpers } from "./feedHelpers.generated";

// Wasm bridge
export type { WasmBridgeLoadResult, WasmBridgeUnavailable, UpdateBytesSink } from "./wasmBridge";
export { WasmBridge, loadWasmBridge } from "./wasmBridge";

// Degraded runtime
export type { DegradedRuntimeMode } from "./degradedRuntime";
export { DegradedRuntime } from "./degradedRuntime";

// Npub encoding utility (calls Rust NIP-19 encoder — never browser bech32)
export { encodeNpub } from "./encodeNpub";
