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
  return command("chirp.react", { target_event_id: targetEventId, reaction });
}

export function followCommand(pubkey: string, following: boolean): RuntimeCommand {
  return command(following ? "chirp.follow" : "chirp.unfollow", { pubkey });
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

function command(actionType: string, payload: unknown): RuntimeCommand {
  return { actionType, payload };
}

function group(hostRelayUrl: string, localId: string) {
  return { host_relay_url: hostRelayUrl, local_id: localId };
}
