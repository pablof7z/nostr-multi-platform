// Protocol types and helpers
export type {
  WorkerRequest,
  WorkerEvent,
  RuntimeStatus,
  IdentityRelayPermission,
  FeedHandle,
} from "./protocol";
export { protocolVersion, eventCorrelationId, labelRuntimeStatus } from "./protocol";

// ADR-0071 typed write transport — the `DispatchEnvelope` byte encoder.
export {
  encodeDispatchEnvelope,
  DISPATCH_ENVELOPE_FILE_IDENTIFIER,
  DISPATCH_ENVELOPE_SCHEMA_VERSION,
} from "./dispatchEnvelope";

// ADR-0071 §3 (#1776) — generated typed write builders. Field-level sugar that
// encodes the per-crate FlatBuffers payload + wraps it in a `DispatchEnvelope`
// for the `dispatch_bytes` doorway, so the host never hand-assembles FlatBuffers
// or spells an `action_namespace`. GENERATED — see
// `crates/nmp-codegen/src/action_builders/registry.rs`.
export { GeneratedActionBuilders } from "./actionBuilders.generated";

// #1626 generated typed feed helpers. Field-level sugar that builds canonical
// FeedParams JSON and targets the `feed_open_json` Worker control; Rust owns the
// feed compiler, source graph, and teardown.
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

// #2722 — NIP-07 main-thread sign-request round-trip. Fulfils the
// `begin_sign` / `sign_request` round-trip `protocol.ts` documents; the
// browser boundary (#2082) means this module never calls a signer extension
// directly — the host injects a `Nip07Signer` adapter over its own extension
// access.
export type { Nip07Signer, SignRequest } from "./signBroker";
export {
  asSignRequest,
  deliverSignerResponse,
  fulfilSignRequest,
  installNip07SignBroker,
} from "./signBroker";

// #2722 — read-side UpdateFrame decode. Pure decode of the `NMPU` bytes the
// `update_bytes` worker event carries into the declared projection-key set
// plus a typed-sidecar lookup, verified against the GENERATED
// `PROJECTION_CONTRACT` (see below) rather than a hand-copied file identifier
// per call site.
export type { DecodedUpdateFrame } from "./updateFrameDecoder";
export { decodeUpdateFrame } from "./updateFrameDecoder";

// #2722 — the neutral wire identity (schema_id + file_identifier) of every
// projection the kernel/host can emit. GENERATED — see
// `crates/nmp-codegen/src/ts_projection_contract.rs`.
export type { ProjectionContractEntry } from "./projectionContract.generated";
export { PROJECTION_CONTRACT } from "./projectionContract.generated";

// #2722 — the keyed per-row reference cache (`refs.profile` / `refs.event`),
// the TypeScript twin of the generated Swift/Kotlin `KeyedRefCache`. GENERATED
// — see `crates/nmp-codegen/src/ts_keyed_cache.rs`. The typed `profile(...)`
// / `profiles()` accessors replace hl's hand-written `RefProfileStore` /
// `RefRowCache` pair with one class sourced from the same registry as native.
export type { RefRowApplyOutcome } from "./keyedRefCache.generated";
export { KeyedRefCache } from "./keyedRefCache.generated";
export type { ProfileWire } from "./refRowDecoders";
