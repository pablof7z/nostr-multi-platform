export type WorkerRequest =
  | { type: "hello"; app_id: string; platform: string; protocol_version: number }
  | {
      type: "start";
      app_id: string;
      /** Relay set the host wants the runtime to connect to. Relay policy is a
       *  host concern (#1125): the framework defines no defaults, so the host
       *  MUST supply this. */
      relays: string[];
      /** Explicit relay bootstrap list (url + role). Host policy with no
       *  framework default — the host MUST supply it. When non-empty the wasm
       *  runtime uses it verbatim and ignores `relays`. */
      relay_bootstrap: { url: string; role: string }[];
      database_name: string;
      correlation_id: string;
    }
  /** ADR-0063 structured reference-resolution control. This is not an app
   *  write doorway and cannot carry arbitrary action namespaces. */
  | {
      type: "resolve_ref";
      namespace: number;
      key: string;
      consumer_id: string;
      shape: number;
      liveness: number;
      /** Optional relay hints decoded from NIP-19/NIP-21 event refs. */
      hints?: string[];
      /** Optional nevent author TLV decoded at the app boundary. */
      event_author?: string | null;
      correlation_id: string;
    }
  /** ADR-0063 structured reference release. */
  | {
      type: "release_ref";
      namespace: number;
      key: string;
      consumer_id: string;
      correlation_id: string;
    }
  /** ADR-0064 / S2 (#1750) — the typed binary write doorway. `bytes` are a
   *  finished `DispatchEnvelope` FlatBuffers root (file id `NMPD`) carrying the
   *  correlation_id + generated action_namespace + opaque typed payload. This
   *  is the ONLY app-level write path (the hand-rolled `app_action` envelope was
   *  deleted in #1743 Cut A); it is identical in shape to the native FFI seam. */
  | {
      type: "dispatch_bytes";
      bytes: Uint8Array;
    }
  | {
      type: "capability_result";
      capability: string;
      correlation_id: string;
      payload: unknown;
    }
  | { type: "stop"; correlation_id: string }
  /** Set the active identity. The browser host calls window.nostr.getPublicKey()
   *  first, then sends this request so the wasm runtime can seed the kernel's
   *  active account. kind: "nip07" (the only kind wired). ADR-0064 §5: this does
   *  NOT install a persistent signer — signing is the begin_sign capability
   *  round-trip. */
  | {
      type: "set_identity";
      kind: string;
      pubkey_hex: string;
      correlation_id: string;
    }
  /** #1753 S6 — begin a NIP-07 sign capability round-trip. The worker parks a
   *  sign op (ADR-0050 §D1) and emits a `sign_request` event the MAIN THREAD
   *  fulfils via window.nostr.signEvent (Workers have no window.nostr). Pure
   *  message re-entry: no polling (D8). */
  | {
      type: "begin_sign";
      account_pubkey: string;
      unsigned_json: string;
    }
  /** #1753 S6 — the main-thread broker delivers the signer response (the
   *  `sign`-verb fulfiller feeding ADR-0050 §D3b). Exactly one of `signed_json`
   *  / `error` is set. Account-pinned: the worker rejects a signature authored
   *  by a different account than the round-trip was begun for. */
  | {
      type: "deliver_signer_response";
      correlation_id: string;
      signed_json?: string | null;
      error?: string | null;
    }
  /** Read-only diagnostics request: return the kernel-owned bounded routing
   *  trace JSON. This is not a write doorway and carries no routing policy in
   *  the host; the worker calls the wasm runtime's existing
   *  `recent_routing_decisions()` accessor. */
  | {
      type: "routing_decisions";
      correlation_id: string;
    };

export type RuntimeStatus =
  | "ready"
  | "running"
  | "stopped"
  | {
      degraded:
        | "browser_actor_driver_missing"
        | "browser_bridge_unavailable"
        | "capability_rejected"
        | "protocol_mismatch";
    };

export type WorkerEvent =
  | { type: "hello_accepted"; protocol_version: number; status: RuntimeStatus }
  | { type: "runtime_status"; status: RuntimeStatus; correlation_id?: string }
  | { type: "action_accepted"; action_type: string; correlation_id: string }
  | { type: "update_bytes"; bytes: Uint8Array }
  | {
      type: "capability_failure";
      capability: string;
      correlation_id: string;
      reason: string;
    }
  /** #1753 S6 — a sign capability request the worker emits for the MAIN-THREAD
   *  broker to fulfil: call window.nostr.signEvent(unsigned_json) (ensuring the
   *  extension is on account_pubkey first) and post back a
   *  `deliver_signer_response` carrying this correlation_id. */
  | {
      type: "sign_request";
      correlation_id: string;
      account_pubkey: string;
      unsigned_json: string;
    }
  /** #1753 S6 — a sign round-trip completed via message re-entry. For publish
   *  dispatches the worker also consumes this completion to publish through the
   *  Rust kernel; the main thread never publishes inline. */
  | { type: "sign_completed"; correlation_id: string; signed_json: string }
  /** #1753 S6 — a sign round-trip failed (parse, account-pin mismatch, user
   *  rejection, or unknown/stale correlation id). */
  | { type: "sign_failed"; correlation_id: string; reason: string }
  /** #968 — read-only routing diagnostics snapshot, JSON rendered by Rust from
   *  the kernel-owned `RoutingTraceProjection`. */
  | { type: "routing_decisions"; correlation_id: string; json: string }
  | { type: "error"; code: string; message: string; correlation_id?: string };

export type WorkerEventSummary = {
  type: WorkerEvent["type"];
  correlation_id?: string;
  status?: RuntimeStatus;
  protocol_version?: number;
  action_type?: string;
  byte_length?: number;
  revision?: string;
  capability?: string;
  reason_prefix?: string;
  code?: string;
  message_prefix?: string;
  signer_account_prefix?: string;
  unsigned_json_bytes?: number;
  signed_json_bytes?: number;
  routing_json_bytes?: number;
  payload?: "redacted";
};

export type ChirpAction =
  | { action: "publish_note"; content: string; reply_to_id?: string | null }
  | { action: "react"; target_event_id: string; reaction?: string }
  | { action: "follow"; pubkey: string }
  | { action: "unfollow"; pubkey: string };

export const protocolVersion = 1;
const LOG_PREFIX_CHARS = 96;

export function summarizeWorkerEvent(
  event: WorkerEvent,
  metadata: { revision?: bigint } = {},
): WorkerEventSummary {
  switch (event.type) {
    case "hello_accepted":
      return {
        type: event.type,
        protocol_version: event.protocol_version,
        status: event.status,
      };
    case "runtime_status":
      return {
        type: event.type,
        correlation_id: event.correlation_id,
        status: event.status,
      };
    case "action_accepted":
      return {
        type: event.type,
        correlation_id: event.correlation_id,
        action_type: event.action_type,
      };
    case "update_bytes":
      return {
        type: event.type,
        byte_length: event.bytes.byteLength,
        revision: metadata.revision?.toString(),
        payload: "redacted",
      };
    case "capability_failure":
      return {
        type: event.type,
        correlation_id: event.correlation_id,
        capability: event.capability,
        reason_prefix: logPrefix(event.reason),
      };
    case "sign_request":
      return {
        type: event.type,
        correlation_id: event.correlation_id,
        signer_account_prefix: pubkeyPrefix(event.account_pubkey),
        unsigned_json_bytes: utf8ByteLength(event.unsigned_json),
        payload: "redacted",
      };
    case "sign_completed":
      return {
        type: event.type,
        correlation_id: event.correlation_id,
        signed_json_bytes: utf8ByteLength(event.signed_json),
        payload: "redacted",
      };
    case "sign_failed":
      return {
        type: event.type,
        correlation_id: event.correlation_id,
        reason_prefix: logPrefix(event.reason),
      };
    case "routing_decisions":
      return {
        type: event.type,
        correlation_id: event.correlation_id,
        routing_json_bytes: utf8ByteLength(event.json),
        payload: "redacted",
      };
    case "error":
      return {
        type: event.type,
        correlation_id: event.correlation_id,
        code: event.code,
        message_prefix: logPrefix(event.message),
      };
  }
}

function logPrefix(value: string): string {
  return value.length > LOG_PREFIX_CHARS ? `${value.slice(0, LOG_PREFIX_CHARS)}...` : value;
}

function pubkeyPrefix(value: string): string {
  return value.length > 8 ? `${value.slice(0, 8)}...` : value;
}

function utf8ByteLength(value: string): number {
  let length = 0;
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code < 0x80) {
      length += 1;
    } else if (code < 0x800) {
      length += 2;
    } else if (code >= 0xd800 && code <= 0xdbff && i + 1 < value.length) {
      const next = value.charCodeAt(i + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        length += 4;
        i += 1;
      } else {
        length += 3;
      }
    } else {
      length += 3;
    }
  }
  return length;
}

export function eventCorrelationId(event: WorkerEvent): string | undefined {
  switch (event.type) {
    case "runtime_status":
    case "action_accepted":
    case "error":
      return event.correlation_id;
    case "capability_failure":
      return event.correlation_id;
    // #1753 S6 — sign round-trip terminals are correlation-keyed so a pending
    // request resolves on completion; `sign_request` is NOT (it is a broker
    // instruction the main thread acts on, not a reply to a pending caller).
    case "sign_completed":
    case "sign_failed":
    case "routing_decisions":
      return event.correlation_id;
    case "sign_request":
    case "hello_accepted":
    case "update_bytes":
      return undefined;
  }
}

export function labelRuntimeStatus(status: RuntimeStatus): string {
  if (typeof status === "string") {
    return status.replace("_", " ");
  }
  return status.degraded.split("_").join(" ");
}
