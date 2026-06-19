export type WorkerRequest =
  | { type: "hello"; app_id: string; platform: string; protocol_version: number }
  | {
      type: "start";
      app_id: string;
      relays?: string[];
      /** Explicit relay bootstrap list; when non-empty the wasm runtime uses
       *  it verbatim and ignores `relays`. Used by the Playwright boot smoke
       *  to point the runtime at the fixture relay instead of the chirp defaults. */
      relay_bootstrap?: { url: string; role: string }[];
      database_name: string;
      correlation_id: string;
    }
  | {
      type: "dispatch";
      action_type: string;
      payload: unknown;
      correlation_id: string;
    }
  | {
      type: "app_action";
      action: ChirpAction;
      correlation_id: string;
    }
  | {
      type: "capability_result";
      capability: string;
      correlation_id: string;
      payload: unknown;
    }
  | { type: "stop"; correlation_id: string }
  /** V-01 Stage 3b — install a NIP-07 signer.
   *  The browser host calls window.nostr.getPublicKey() first, then sends
   *  this request so the wasm runtime's install path stays synchronous.
   *  kind: "nip07" (the only kind wired in Stage 3b). */
  | {
      type: "set_signer";
      kind: string;
      pubkey_hex: string;
      correlation_id: string;
    }
  /** Encode a hex pubkey to npub via the canonical Rust NIP-19 encoder in the
   *  wasm module (aim.md §6.9 — never bech32-encode in the browser). */
  | {
      type: "encode_npub";
      pubkey: string;
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
  /** Response to `encode_npub`. `npub`/`npubShort` are Rust-encoded; both are
   *  absent (undefined) when the pubkey was invalid. */
  | {
      type: "npub_encoded";
      pubkey: string;
      npub?: string;
      npubShort?: string;
      correlation_id: string;
    }
  | { type: "error"; code: string; message: string; correlation_id?: string };

export type ChirpAction =
  | { action: "publish_note"; content: string; reply_to_id?: string | null }
  | { action: "react"; target_event_id: string; reaction?: string }
  | { action: "follow"; pubkey: string }
  | { action: "unfollow"; pubkey: string };

export const protocolVersion = 1;

export function eventCorrelationId(event: WorkerEvent): string | undefined {
  switch (event.type) {
    case "runtime_status":
    case "action_accepted":
    case "error":
      return event.correlation_id;
    case "capability_failure":
      return event.correlation_id;
    case "npub_encoded":
      return event.correlation_id;
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
