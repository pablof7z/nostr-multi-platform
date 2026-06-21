// Gallery worker protocol — re-exports from @nmp/runtime-web.
// The gallery has no app-specific worker request/event types beyond the base
// protocol. npub encoding is no longer a worker message — use the encodeNpub()
// pure utility from @nmp/runtime-web directly on the main thread.
export type {
  WorkerRequest,
  WorkerEvent,
  RuntimeStatus,
} from "@nmp/runtime-web";
export {
  protocolVersion,
  eventCorrelationId,
  labelRuntimeStatus,
} from "@nmp/runtime-web";
