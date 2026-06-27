import { finalizeEvent, generateSecretKey, getPublicKey, nip17 } from "nostr-tools";
import {
  type FixtureRelay,
  type NostrEvent,
  startFixtureRelayServer,
} from "./fixture-relay.js";

export type MessagesFixtureRelay = FixtureRelay & {
  viewerSecretKey: Uint8Array;
  viewerPubkey: string;
  senderSecretKey: Uint8Array;
  senderPubkey: string;
  messageContent: string;
};

export async function startMessagesFixtureRelay(): Promise<MessagesFixtureRelay> {
  const viewerSk = generateSecretKey();
  const viewerPubkey = getPublicKey(viewerSk);
  const senderSk = generateSecretKey();
  const senderPubkey = getPublicKey(senderSk);
  const messageContent = "private hello from fixture relay";
  const seeded: NostrEvent[] = [];
  const base = await startFixtureRelayServer(seeded, { secure: true });
  const now = Math.floor(Date.now() / 1000);
  const relayList = finalizeEvent(
    {
      kind: 10050,
      created_at: now - 30,
      tags: [["relay", base.url]],
      content: "",
    },
    viewerSk,
  ) as NostrEvent;
  const senderRelayList = finalizeEvent(
    {
      kind: 10050,
      created_at: now - 25,
      tags: [["relay", base.url]],
      content: "",
    },
    senderSk,
  ) as NostrEvent;
  const giftWrap = nip17.wrapEvent(
    senderSk,
    { publicKey: viewerPubkey },
    messageContent,
  ) as NostrEvent;
  seeded.push(relayList, senderRelayList, giftWrap);

  return {
    ...base,
    viewerSecretKey: viewerSk,
    viewerPubkey,
    senderSecretKey: senderSk,
    senderPubkey,
    messageContent,
  };
}
