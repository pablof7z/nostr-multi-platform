import { decodeDispatchEnvelopeRouting } from "./dispatchEnvelope";
import type { RuntimeStatus, WorkerEvent, WorkerRequest } from "./protocol";

export type DegradedRuntimeMode = "browser_actor_driver_missing" | "browser_bridge_unavailable";

export class DegradedRuntime {
  private status: RuntimeStatus = "ready";

  constructor(
    private readonly mode: DegradedRuntimeMode = "browser_actor_driver_missing",
    private readonly unavailableReason = "nmp-wasm actor driver is not linked into the web worker yet",
  ) {}

  /** Uniform with {@link WasmBridge.prepareForStart}: the degraded runtime has no
   *  durable store to open, so this is always a no-op. Lets the worker call
   *  `prepareForStart` on either runtime without a type narrow. */
  async prepareForStart(_request: WorkerRequest): Promise<void> {
    // no durable backend in degraded mode
  }

  handle(request: WorkerRequest): WorkerEvent[] {
    switch (request.type) {
      case "hello":
        return this.hello(request.protocol_version);
      case "start":
        this.status = { degraded: this.mode };
        return [
          {
            type: "runtime_status",
            status: this.status,
            correlation_id: request.correlation_id,
          },
        ];
      case "resolve_ref":
        return [
          {
            type: "capability_failure",
            capability: "nmp.kernel.resolve_ref",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "release_ref":
        return [
          {
            type: "capability_failure",
            capability: "nmp.kernel.release_ref",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "dispatch_bytes": {
        // Decode only the routing fields (correlation_id + action_namespace);
        // the opaque payload is never interpreted. A buffer that is not a
        // DispatchEnvelope root fails closed with an honest error (D6).
        const routing = decodeDispatchEnvelopeRouting(request.bytes);
        if (!routing) {
          return [
            {
              type: "error",
              code: "dispatch_envelope_rejected",
              message: "dispatch_bytes did not carry a DispatchEnvelope root (bad file identifier)",
            },
          ];
        }
        return [
          {
            type: "capability_failure",
            capability: routing.actionNamespace,
            correlation_id: routing.correlationId,
            reason: this.unavailableReason,
          },
        ];
      }
      case "search_open":
        return [
          {
            type: "capability_failure",
            capability: "nmp.nip50.search.open",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "search_close":
        return [
          {
            type: "capability_failure",
            capability: "nmp.nip50.search.close",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "group_discovery_open":
        return [
          {
            type: "capability_failure",
            capability: "nmp.nip29.group_discovery.open",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "group_discovery_close":
        return [
          {
            type: "capability_failure",
            capability: "nmp.nip29.group_discovery.close",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "group_timeline_open":
        return [
          {
            type: "capability_failure",
            capability: "nmp.nip29.group_timeline.open",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "group_timeline_close":
        return [
          {
            type: "capability_failure",
            capability: "nmp.nip29.group_timeline.close",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "notifications_open":
        return [
          {
            type: "capability_failure",
            capability: "nmp.relations.notifications.open",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "notifications_close":
        return [
          {
            type: "capability_failure",
            capability: "nmp.relations.notifications.close",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "notifications_mark_read":
        return [
          {
            type: "capability_failure",
            capability: "nmp.relations.notifications.mark_read",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "relay_config":
        return [
          {
            type: "capability_failure",
            capability: "nmp.relay_config",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "publish_relay_preferences":
        return [
          {
            type: "capability_failure",
            capability: "nmp.nip65.publish_relay_list",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "capability_result":
        return [
          {
            type: "capability_failure",
            capability: request.capability,
            correlation_id: request.correlation_id,
            reason: "capability completions require a running actor",
          },
        ];
      case "set_identity":
        return [
          {
            type: "capability_failure",
            capability: "set_identity",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      // #1753 S6 — the degraded runtime has no kernel to park a sign op in, so a
      // sign round-trip fails closed (D6). `begin_sign` mints no correlation id;
      // `deliver_signer_response` echoes the one it carried.
      case "begin_sign":
        return [
          {
            type: "sign_failed",
            correlation_id: "",
            reason: this.unavailableReason,
          },
        ];
      case "deliver_signer_response":
        return [
          {
            type: "sign_failed",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "routing_decisions":
        return [
          {
            type: "capability_failure",
            capability: "nmp.routing_decisions",
            correlation_id: request.correlation_id,
            reason: this.unavailableReason,
          },
        ];
      case "stop":
        this.status = "stopped";
        return [
          {
            type: "runtime_status",
            status: this.status,
            correlation_id: request.correlation_id,
          },
        ];
    }
  }

  private hello(version: number): WorkerEvent[] {
    if (version !== 1) {
      return [
        {
          type: "error",
          code: "protocol_mismatch",
          message: `expected protocol 1, got ${version}`,
        },
      ];
    }
    return [{ type: "hello_accepted", protocol_version: 1, status: this.status }];
  }
}
