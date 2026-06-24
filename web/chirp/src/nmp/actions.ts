import { GeneratedActionBuilders, type ChirpAction, type WorkerRequest } from "@nmp/runtime-web";

/** An app-level write command routed through `client.dispatchCommand()`.
 *
 * After #1008, typed-write commands (publish, follow, react, etc.) carry a
 * `buildDispatchBytes` factory instead of a JSON payload. When present, the
 * client generates a `correlationId`, calls the factory with it, and sends the
 * result as `WorkerRequest::DispatchBytes` with FlatBuffers payload — bypassing
 * the JSON encode/decode path. Commands that don't carry `buildDispatchBytes`
 * (kernel ops, view ops, wallet ops) route through the legacy JSON `dispatch`
 * arm as before.
 *
 * `buildDispatchBytes(correlationId)` returns the FULL `DispatchEnvelope`
 * bytes as built by `GeneratedActionBuilders`. The client owns the
 * `correlationId` generation and passes it to the builder at dispatch time. */
export type RuntimeCommand = {
  actionType: string;
  payload: unknown;
  /** Typed FlatBuffers write factory (#1008 / ADR-0064). When present, the
   *  client generates a `correlationId`, calls this factory to get the
   *  finished `DispatchEnvelope` bytes, and sends them as
   *  `WorkerRequest::DispatchBytes`. `dispatchBytes` factories are set by
   *  the typed-write command builders (`publishProfileCommand`,
   *  `followCommand`, `reactCommand`, `unfollowCommand`). */
  buildDispatchBytes?: (correlationId: string) => Uint8Array;
};

export function publishNoteAction(content: string, replyToId: string | null = null): ChirpAction {
  return {
    action: "publish_note",
    content,
    reply_to_id: replyToId,
  };
}

// ── ADR-0064 typed write lowering (#1008 / #1743 Cut A) ─────────────────────
//
// A Chirp app-level write (`ChirpAction`) crosses the wasm boundary through the
// ONE typed `dispatch_bytes` doorway carrying a FlatBuffers `DispatchEnvelope`
// — identical in shape to the native FFI seam. After #1008, the payload is a
// proper FlatBuffers buffer built by `GeneratedActionBuilders`, NOT a JSON-
// encoded string. The JSON encoding path is removed (#1008 AC #3): every
// `TYPED_WRITE_NAMESPACE` action MUST use the generated builder.
//
// The `action_namespace` is a GENERATED discriminant embedded in the builder
// output — no human spells it at a call site. This file is the only lowering
// seam; the hand-rolled `app_action` envelope + `AppAction` enum were deleted
// in #1743 Cut A.

/** Build the `dispatch_bytes` worker request for a Chirp `ChirpAction`: lower it
 *  to the correct `GeneratedActionBuilders` call, returning a
 *  `WorkerRequest::DispatchBytes` carrying a proper FlatBuffers envelope.
 *
 *  After #1008 the payload is encoded via generated FlatBuffers builders (NOT
 *  JSON.stringify). Each `ChirpAction` variant maps 1:1 to its builder. */
export function chirpActionRequest(action: ChirpAction, correlationId: string): WorkerRequest {
  let bytes: Uint8Array;
  switch (action.action) {
    case "publish_note":
      // NIP-10 reply-tag construction belongs to the host (#906): a kind:1 note
      // lowers to the engine-generic `PublishRaw`. `reply_to_id` is resolved by
      // the publish path, not forwarded into the envelope.
      bytes = GeneratedActionBuilders.publishRaw(correlationId, 1, [], action.content);
      break;
    case "react":
      bytes = GeneratedActionBuilders.react(
        correlationId,
        action.target_event_id,
        action.reaction ?? "+",
        null,
      );
      break;
    case "follow":
      bytes = GeneratedActionBuilders.follow(correlationId, action.pubkey);
      break;
    case "unfollow":
      bytes = GeneratedActionBuilders.unfollow(correlationId, action.pubkey);
      break;
  }
  return { type: "dispatch_bytes", bytes };
}

export function publishProfileCommand(fields: Record<string, string>): RuntimeCommand {
  // #1008: typed-write command — builds FlatBuffers envelope via generated builder.
  const entries = Object.entries(fields) as Array<[string, string]>;
  return {
    actionType: "nmp.publish",
    payload: { PublishProfile: { fields } },
    buildDispatchBytes: (correlationId) =>
      GeneratedActionBuilders.publishProfile(correlationId, entries),
  };
}

export function reactCommand(targetEventId: string, reaction = "+"): RuntimeCommand {
  // #1008: typed-write command.
  return {
    actionType: "nmp.nip25.react",
    payload: { target_event_id: targetEventId, reaction },
    buildDispatchBytes: (correlationId) =>
      GeneratedActionBuilders.react(correlationId, targetEventId, reaction, null),
  };
}

export function followCommand(pubkey: string, following: boolean): RuntimeCommand {
  // #1008: typed-write command.
  const ns = following ? "nmp.follow" : "nmp.unfollow";
  const builder = following ? GeneratedActionBuilders.follow : GeneratedActionBuilders.unfollow;
  return {
    actionType: ns,
    payload: { pubkey },
    buildDispatchBytes: (correlationId) => builder(correlationId, pubkey),
  };
}

export function openProfileCommand(pubkey: string): RuntimeCommand {
  return command("nmp.view.profile", { pubkey });
}

export function openThreadCommand(eventId: string): RuntimeCommand {
  return command("nmp.view.thread", { event_id: eventId });
}

export function openTagCommand(tag: string): RuntimeCommand {
  return command("nmp.view.tag", { tag });
}

export function sendDmCommand(recipientPubkey: string, content: string): RuntimeCommand {
  return command("nmp.nip17.send", { recipient_pubkey: recipientPubkey, content });
}

export function publishDmRelayListCommand(relays: string[]): RuntimeCommand {
  return command("nmp.nip17.publish_relay_list", { relays });
}

export function discoverGroupsCommand(relayUrl: string): RuntimeCommand {
  return command("nmp.nip29.discover", { relay_url: relayUrl });
}

export function joinGroupCommand(hostRelayUrl: string, localId: string): RuntimeCommand {
  return command("nmp.nip29.join", { group: group(hostRelayUrl, localId) });
}

export function postGroupMessageCommand(hostRelayUrl: string, localId: string, content: string): RuntimeCommand {
  return command("nmp.nip29.post_chat_message", { group: group(hostRelayUrl, localId), content });
}

export function replyGroupMessageCommand(
  hostRelayUrl: string,
  localId: string,
  parentEventId: string,
  content: string,
): RuntimeCommand {
  return command("nmp.nip29.comment_in_group", {
    group: group(hostRelayUrl, localId),
    parent_event_id: parentEventId,
    content,
  });
}

export function reactGroupMessageCommand(
  hostRelayUrl: string,
  localId: string,
  targetEventId: string,
  reaction = "+",
): RuntimeCommand {
  return command("nmp.nip29.react_in_group", {
    group: group(hostRelayUrl, localId),
    target_event_id: targetEventId,
    content: reaction,
  });
}

export function identityCommand(action: string, payload: Record<string, unknown>): RuntimeCommand {
  return command(`nmp.identity.${action}`, payload);
}

export function relayCommand(action: string, payload: Record<string, unknown>): RuntimeCommand {
  return command(`nmp.relay.${action}`, payload);
}

export function outboxCommand(action: "retry" | "cancel", handle: string): RuntimeCommand {
  return command(`nmp.publish.${action}`, { handle });
}

export function walletCommand(action: string, payload: Record<string, unknown> = {}): RuntimeCommand {
  return command(`nmp.wallet.${action}`, payload);
}

// ── ADR-0063 component-owned reference-resolution seam (#1671) ───────────────
//
// Web components call these on mount / unmount to register / release their
// interest in a profile or event through the UNIFIED, origin-blind
// `resolve_ref` / `release_ref` seam (ADR-0063 D1). The kernel refcounts
// consumers per `(namespace, key)`, fetches the entity on the first resolve, and
// emits ONE keyed row-delta projection per namespace (`refs.profile` /
// `refs.event`).
//
// `consumerId` must be STABLE per component instance — e.g.
// `"chirp-web-author-${item.id}"`. Mirror iOS (`chirp-avatar.<uuid>`) and
// Android (`note-author-<eventId>`) naming conventions.
//
// The adapter below is the only place this app spells the wasm ref-dispatch
// wire discriminants. Components call typed helpers (`resolveProfileCommand`,
// `resolveEventCommand`) so they cannot mix a profile namespace with an event
// shape.
// Route via the existing `WorkerRequest::Dispatch` path (`dispatchCommand`).

const refWire = {
  profile: { namespace: 0, shape: { ref: 0, card: 1 } },
  event: { namespace: 1, shape: { embed: 0, raw: 1 } },
  liveness: { cacheOk: 0, live: 1 },
} as const;

/** Resolve a profile reference (feed-avatar `ref` shape, CacheOk). */
export function resolveProfileCommand(pubkey: string, consumerId: string): RuntimeCommand {
  return command("nmp.kernel.resolve_ref", {
    namespace: refWire.profile.namespace,
    key: pubkey,
    consumer_id: consumerId,
    shape: refWire.profile.shape.ref,
    liveness: refWire.liveness.cacheOk,
  });
}

/** Release a profile reference. */
export function releaseProfileCommand(pubkey: string, consumerId: string): RuntimeCommand {
  return command("nmp.kernel.release_ref", {
    namespace: refWire.profile.namespace,
    key: pubkey,
    consumer_id: consumerId,
  });
}

/** Resolve an event reference by raw event key (embed shape, CacheOk). */
export function resolveEventCommand(key: string, consumerId: string): RuntimeCommand {
  return command("nmp.kernel.resolve_ref", {
    namespace: refWire.event.namespace,
    key,
    consumer_id: consumerId,
    shape: refWire.event.shape.embed,
    liveness: refWire.liveness.cacheOk,
  });
}

/** Release an event reference. */
export function releaseEventCommand(key: string, consumerId: string): RuntimeCommand {
  return command("nmp.kernel.release_ref", {
    namespace: refWire.event.namespace,
    key,
    consumer_id: consumerId,
  });
}

function command(actionType: string, payload: unknown): RuntimeCommand {
  return { actionType, payload };
}

function group(hostRelayUrl: string, localId: string) {
  return { host_relay_url: hostRelayUrl, local_id: localId };
}
