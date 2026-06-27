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
  /** NIP-50 public search. The worker validates/bounds the query through
   *  `nmp_nip50::SearchRequest` and emits typed results under
   *  `nmp.nip50.search.<session_id>` (`N50S`) in update snapshots. */
  | {
      type: "search_open";
      session_id: string;
      query: string;
      scope: "notes" | "profiles" | "longform";
      targets: "user_preferred" | "app_default" | "explicit";
      relays?: string[];
      max_hits?: number;
      correlation_id: string;
    }
  | {
      type: "search_close";
      session_id: string;
      correlation_id: string;
    }
  /** NIP-29 public group discovery. The worker owns the relay-pinned metadata
   *  interest and emits `nmp.nip29.discovered_groups` (`NDGS`) in snapshots. */
  | {
      type: "group_discovery_open";
      session_id: string;
      relay_url: string;
      correlation_id: string;
    }
  | {
      type: "group_discovery_close";
      session_id: string;
      correlation_id: string;
    }
  /** Browser runtime relay inventory edit. This is structured transport/runtime
   *  control, not an app-level write. The Rust runtime validates URL/role,
   *  mutates the configured-relay projection, and opens/closes browser relay
   *  drivers so diagnostics reflect the live socket inventory. */
  | {
      type: "relay_config";
      action: "add" | "remove";
      url: string;
      role?: string;
      correlation_id: string;
    }
  /** Publish the Rust-owned configured relay projection as NIP-65 kind:10002.
   *  This is a typed write: the worker encodes `nmp.nip65.publish_relay_list`
   *  and routes it through the same `dispatch_bytes` path, so TS never builds
   *  NIP-65 tags or marker policy. */
  | {
      type: "publish_relay_preferences";
      correlation_id: string;
    }
  | {
      type: "capability_result";
      capability: string;
      correlation_id: string;
      payload: unknown;
    }
  | { type: "stop"; correlation_id: string }
  /** Set the active identity. Signer kinds:
   *
   *  kind: "nip07" — the browser host called window.nostr.getPublicKey() first
   *    and provides pubkey_hex. Signing is a begin_sign capability round-trip
   *    routed back to the main thread (ADR-0064 §5). Does NOT install a
   *    persistent signer. This is the path the current nmp-browser-runtime
   *    wires end-to-end.
   *
   *  kind: "local_key" — the host provides secret_key_bech32 (the raw nsec
   *    bech32 string) and an empty pubkey_hex; the runtime is responsible for
   *    decoding the nsec, deriving the pubkey, and installing a LocalKey
   *    provider. The TS layer MUST NOT decode or sign with the nsec — handing
   *    it to the runtime is the only permitted use.
   */
  | {
      type: "set_identity";
      kind: string;
      /** Hex pubkey. Carries the NIP-07 pubkey for kind: "nip07"; sent as an
       *  empty string for kind: "local_key" (the runtime derives it from the
       *  secret key). Always present so the request deserializes against the
       *  runtime's required field. */
      pubkey_hex: string;
      /** For kind: "local_key" — raw nsec bech32 string handed to the runtime's
       *  LocalKey provider. Never decoded or signed-with in TS. */
      secret_key_bech32?: string;
      correlation_id: string;
      /** Raw relay permissions reported by the identity backend, e.g. NIP-07
       *  getRelays(). The worker owns canonicalization and role mapping. */
      identity_relays?: IdentityRelayPermission[];
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

export type IdentityRelayPermission = {
  url: string;
  read?: boolean;
  write?: boolean;
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
      /** Present when this sign request was caused by an action dispatch; lets
       *  the UI settle the initiating command while the signer broker continues. */
      action_correlation_id?: string | null;
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

export const protocolVersion = 1;

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
