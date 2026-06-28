import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";
import {
  type FixtureRelay,
  type NostrEvent,
  startFixtureRelayServer,
} from "./fixture-relay.js";

export type NotificationsFixtureRelay = FixtureRelay & {
  viewerSecretKey: Uint8Array;
  viewerPubkey: string;
  replyContent: string;
  mentionContent: string;
  actorPubkey: string;
};

export async function startNotificationsFixtureRelay(): Promise<NotificationsFixtureRelay> {
  const viewerSk = generateSecretKey();
  const viewerPubkey = getPublicKey(viewerSk);
  const actorSk = generateSecretKey();
  const actorPubkey = getPublicKey(actorSk);
  const targetSk = generateSecretKey();
  const targetPubkey = getPublicKey(targetSk);
  const now = Math.floor(Date.now() / 1000);
  const replyContent = "reply notification from fixture relay";
  const mentionContent = "mention notification from fixture relay";

  const targetNote = finalizeEvent(
    {
      kind: 1,
      created_at: now - 60,
      tags: [["p", viewerPubkey]],
      content: "viewer target note",
    },
    targetSk,
  ) as NostrEvent;

  const reply = finalizeEvent(
    {
      kind: 1,
      created_at: now - 40,
      tags: [
        ["e", targetNote.id, "", "reply"],
        ["p", viewerPubkey],
        ["p", targetPubkey],
      ],
      content: replyContent,
    },
    actorSk,
  ) as NostrEvent;

  const mention = finalizeEvent(
    {
      kind: 1,
      created_at: now - 30,
      tags: [["p", viewerPubkey]],
      content: mentionContent,
    },
    actorSk,
  ) as NostrEvent;

  const reaction = finalizeEvent(
    {
      kind: 7,
      created_at: now - 20,
      tags: [
        ["e", targetNote.id],
        ["p", viewerPubkey],
      ],
      content: "+",
    },
    actorSk,
  ) as NostrEvent;

  const repost = finalizeEvent(
    {
      kind: 6,
      created_at: now - 10,
      tags: [
        ["e", targetNote.id],
        ["p", viewerPubkey],
        ["k", "1"],
      ],
      content: "",
    },
    actorSk,
  ) as NostrEvent;

  const base = await startFixtureRelayServer([targetNote, reply, mention, reaction, repost]);
  return {
    ...base,
    viewerSecretKey: viewerSk,
    viewerPubkey,
    replyContent,
    mentionContent,
    actorPubkey,
  };
}
