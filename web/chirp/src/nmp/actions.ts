import type { ChirpAction } from "./protocol";

export type RuntimeCommand = {
  actionType: string;
  payload: unknown;
};

export function publishNoteAction(content: string, replyToId: string | null = null): ChirpAction {
  return {
    action: "publish_note",
    content,
    reply_to_id: replyToId,
  };
}

export function publishProfileCommand(fields: Record<string, string>): RuntimeCommand {
  return command("nmp.publish", { PublishProfile: { fields } });
}

export function reactCommand(targetEventId: string, reaction = "+"): RuntimeCommand {
  return command("nmp.nip25.react", { target_event_id: targetEventId, reaction });
}

export function followCommand(pubkey: string, following: boolean): RuntimeCommand {
  return command(following ? "nmp.follow" : "nmp.unfollow", { pubkey });
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
// `resolve_ref` / `release_ref` seam (ADR-0063 D1) — the generalisation of the
// former `claim_profile` / `claim_event` surface. The kernel refcounts
// consumers per `(namespace, key)`, fetches the entity on the first resolve, and
// emits ONE keyed row-delta projection per namespace (`refs.profile` /
// `refs.event`).
//
// `consumerId` must be STABLE per component instance — e.g.
// `"chirp-web-author-${item.id}"`. Mirror iOS (`chirp-avatar.<uuid>`) and
// Android (`note-author-<eventId>`) naming conventions.
//
// The JSON payload mirrors the Lane D FFI integer codes (apps/chirp/chirp-tui
// runtime.rs) so the wasm dispatch recognizer (`resolve_dispatch_from_action`)
// decodes the same `(namespace, shape, liveness)` the native C-ABI carries:
//   namespace: 0 = profile, 1 = event
//   shape:     profile → 0 = ref (avatar subset), 1 = card (full ProfileCard)
//              event   → 0 = embed, 1 = raw
//   liveness:  0 = CacheOk (background fetch), 1 = Live (tailing sub)
// Route via the existing `WorkerRequest::Dispatch` path (`dispatchCommand`).

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
  return command("nmp.kernel.resolve_ref", {
    namespace: REF_NS_PROFILE,
    key: pubkey,
    consumer_id: consumerId,
    shape: REF_SHAPE_PROFILE_REF,
    liveness: REF_LIVENESS_CACHE_OK,
  });
}

/** Release a profile reference. */
export function releaseProfileCommand(pubkey: string, consumerId: string): RuntimeCommand {
  return command("nmp.kernel.release_ref", {
    namespace: REF_NS_PROFILE,
    key: pubkey,
    consumer_id: consumerId,
  });
}

/** Resolve an event reference by raw event key (embed shape, CacheOk). */
export function resolveEventCommand(key: string, consumerId: string): RuntimeCommand {
  return command("nmp.kernel.resolve_ref", {
    namespace: REF_NS_EVENT,
    key,
    consumer_id: consumerId,
    shape: REF_SHAPE_EVENT_EMBED,
    liveness: REF_LIVENESS_CACHE_OK,
  });
}

/** Release an event reference. */
export function releaseEventCommand(key: string, consumerId: string): RuntimeCommand {
  return command("nmp.kernel.release_ref", {
    namespace: REF_NS_EVENT,
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
