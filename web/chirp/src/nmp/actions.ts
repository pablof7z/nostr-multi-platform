import { GeneratedActionBuilders, type WorkerRequest } from "@nmp/runtime-web";

/** High-level Chirp write actions (app-specific discriminant over dispatch_bytes).
 *
 * These are lowered to typed `dispatch_bytes` via `GeneratedActionBuilders` —
 * no hand-assembled FlatBuffers or Nostr event JSON in TS. */
export type ChirpAction =
  | { action: "publish_note"; content: string; reply_to_id?: string | null }
  | { action: "react"; target_event_id: string; reaction?: string }
  | { action: "follow"; pubkey: string }
  | { action: "unfollow"; pubkey: string };

/** Command routed through `client.dispatchCommand()`.
 *
 * App-level writes are only representable as `dispatch_bytes` commands carrying
 * a generated FlatBuffers `DispatchEnvelope` factory. Reference bookkeeping is
 * represented by structured control commands. Unsupported preview buttons
 * produce local `capability_failure` events; they do not mint arbitrary
 * `action_type + payload` JSON. */
export type RuntimeCommand =
  | {
      kind: "dispatch_bytes";
      actionType: string;
      buildDispatchBytes: (correlationId: string) => Uint8Array;
    }
  | {
      kind: "resolve_ref";
      namespace: number;
      key: string;
      consumerId: string;
      shape: number;
      liveness: number;
      hints?: string[];
      eventAuthor?: string;
    }
  | {
      kind: "release_ref";
      namespace: number;
      key: string;
      consumerId: string;
    }
  | {
      kind: "relay_config";
      action: "add" | "remove";
      url: string;
      role?: string;
    }
  | {
      kind: "unsupported";
      capability: string;
      reason: string;
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
// seam.

/** Build the `dispatch_bytes` worker request for a Chirp `ChirpAction`: lower it
 *  to the correct `GeneratedActionBuilders` call, returning a
 *  `WorkerRequest::DispatchBytes` carrying a proper FlatBuffers envelope.
 *
 *  Each `ChirpAction` variant maps 1:1 to its builder. No protocol logic in TS. */
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
  const entries = Object.entries(fields) as Array<[string, string]>;
  return {
    kind: "dispatch_bytes",
    actionType: "nmp.publish",
    buildDispatchBytes: (correlationId) =>
      GeneratedActionBuilders.publishProfile(correlationId, entries),
  };
}

export function reactCommand(targetEventId: string, reaction = "+"): RuntimeCommand {
  return {
    kind: "dispatch_bytes",
    actionType: "nmp.nip25.react",
    buildDispatchBytes: (correlationId) =>
      GeneratedActionBuilders.react(correlationId, targetEventId, reaction, null),
  };
}

export function followCommand(pubkey: string, following: boolean): RuntimeCommand {
  const ns = following ? "nmp.follow" : "nmp.unfollow";
  const builder = following ? GeneratedActionBuilders.follow : GeneratedActionBuilders.unfollow;
  return {
    kind: "dispatch_bytes",
    actionType: ns,
    buildDispatchBytes: (correlationId) => builder(correlationId, pubkey),
  };
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
// `consumerId` must be STABLE per component instance.
//
// These fields mirror the Lane D FFI integer codes so the wasm `resolve_ref` /
// `release_ref` messages carry the same `(namespace, shape, liveness)` the
// native C-ABI carries:
//   namespace: 0 = profile, 1 = event
//   shape:     profile → 0 = ref (avatar subset), 1 = card (full ProfileCard)
//              event   → 0 = embed, 1 = raw
//   liveness:  0 = CacheOk (background fetch), 1 = Live (tailing sub)

/** Lane D namespace discriminants (mirror `RefNamespace`). */
export const REF_NS_PROFILE = 0;
export const REF_NS_EVENT = 1;
/** profile shapes (mirror `ProfileShape`). */
export const REF_SHAPE_PROFILE_REF = 0;
export const REF_SHAPE_PROFILE_CARD = 1;
/** event shapes (mirror `EventShape`). */
export const REF_SHAPE_EVENT_EMBED = 0;
export const REF_SHAPE_EVENT_RAW = 1;
/** liveness (mirror `RefLiveness`). */
export const REF_LIVENESS_CACHE_OK = 0;
export const REF_LIVENESS_LIVE = 1;

/** Resolve a profile reference (feed-avatar `ref` shape, CacheOk). */
export function resolveProfileCommand(pubkey: string, consumerId: string): RuntimeCommand {
  return {
    kind: "resolve_ref",
    namespace: REF_NS_PROFILE,
    key: pubkey,
    consumerId,
    shape: REF_SHAPE_PROFILE_REF,
    liveness: REF_LIVENESS_CACHE_OK,
  };
}

/** Release a profile reference. */
export function releaseProfileCommand(pubkey: string, consumerId: string): RuntimeCommand {
  return {
    kind: "release_ref",
    namespace: REF_NS_PROFILE,
    key: pubkey,
    consumerId,
  };
}

/** Resolve an event reference by raw event key (embed shape, CacheOk). */
export function resolveEventCommand(
  key: string,
  consumerId: string,
  hints: string[] = [],
  eventAuthor?: string,
): RuntimeCommand {
  const command: RuntimeCommand = {
    kind: "resolve_ref",
    namespace: REF_NS_EVENT,
    key,
    consumerId,
    shape: REF_SHAPE_EVENT_EMBED,
    liveness: REF_LIVENESS_CACHE_OK,
  };
  if (hints.length > 0 && command.kind === "resolve_ref") {
    command.hints = hints;
  }
  if (eventAuthor && command.kind === "resolve_ref") {
    command.eventAuthor = eventAuthor;
  }
  return command;
}

/** Release an event reference. */
export function releaseEventCommand(key: string, consumerId: string): RuntimeCommand {
  return {
    kind: "release_ref",
    namespace: REF_NS_EVENT,
    key,
    consumerId,
  };
}

export function addRelayCommand(url: string, role: string): RuntimeCommand {
  return {
    kind: "relay_config",
    action: "add",
    url,
    role,
  };
}

export function removeRelayCommand(url: string): RuntimeCommand {
  return {
    kind: "relay_config",
    action: "remove",
    url,
  };
}

function unsupportedCommand(capability: string, _payload: unknown): RuntimeCommand {
  return {
    kind: "unsupported",
    capability,
    reason: `unsupported_in_web_preview: ${capability} is not wired to a generated dispatch_bytes builder in Chirp Web yet`,
  };
}

export function openProfileCommand(pubkey: string): RuntimeCommand {
  return unsupportedCommand("nmp.view.profile", { pubkey });
}

export function openThreadCommand(eventId: string): RuntimeCommand {
  return unsupportedCommand("nmp.view.thread", { event_id: eventId });
}

export function openTagCommand(tag: string): RuntimeCommand {
  return unsupportedCommand("nmp.view.tag", { tag });
}

export function sendDmCommand(recipientPubkey: string, content: string): RuntimeCommand {
  return unsupportedCommand("nmp.nip17.send", { recipient_pubkey: recipientPubkey, content });
}

export function outboxCommand(action: "retry" | "cancel", handle: string): RuntimeCommand {
  return unsupportedCommand(`nmp.publish.${action}`, { handle });
}
