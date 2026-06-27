// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform ts \
//       --out web/packages/runtime-web/src/actionBuilders.generated.ts
//
// Source of truth: `crates/nmp-codegen/src/action_builders/registry.rs`
// (`ACTION_BUILDERS`). The CI gate (`.github/workflows/codegen-drift.yml`) fails
// any PR whose generated TypeScript differs from a fresh run.
//
// ADR-0064 §3 (#1776) — typed write builders. Each function below encodes the
// per-crate FlatBuffers payload for one open-registry `action_namespace` and
// stamps it, the namespace, and the envelope schema_version into a
// `DispatchEnvelope`, returning the finished bytes for the `dispatch_bytes` wasm
// doorway (#1750). App code NEVER spells a namespace string or hand-assembles
// FlatBuffers — that lives only here, in generated code. The host supplies the
// `correlationId` (the operation identity end to end, ADR-0064 §4) and owns the
// boundary call.
// ─────────────────────────────────────────────────────────────────────────────

import * as flatbuffers from "flatbuffers";

import { encodeDispatchEnvelope } from "./dispatchEnvelope";

/** Encode a `[string]` FlatBuffers vector (built last element first) and
 * return its offset. Shared by the generated builders below. */
function stringVector(fbb: flatbuffers.Builder, values: string[]): flatbuffers.Offset {
  const offsets = values.map((s) => fbb.createString(s));
  fbb.startVector(4, offsets.length, 4);
  for (let i = offsets.length - 1; i >= 0; i--) fbb.addOffset(offsets[i]!);
  return fbb.endVector();
}

/** Map a relay role string to the RelayMarker ubyte (Both=0, Read=1, Write=2, Indexer=3),
* mirroring `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including rejection.
* Unknown tokens or no-flag input (e.g. empty string) encode as 255 (out-of-range sentinel)
* so the Rust decoder (`marker_from_wire`) fails closed instead of silently becoming Both.
* Role strings may be comma-separated (e.g. `"both,indexer"`); comparisons are case-insensitive. */
function relayMarkerByte(role: string): number {
  let hasBoth = false, hasRead = false, hasWrite = false, hasIndexer = false;
  let invalid = false;
  for (const part of role.split(",").map((s) => s.trim().toLowerCase())) {
    if (part === "") { /* no-op: empty part (e.g. trailing comma) matches Rust */ }
    else if (part === "both") hasBoth = true;
    else if (part === "read") hasRead = true;
    else if (part === "write") hasWrite = true;
    else if (part === "indexer") hasIndexer = true;
    else invalid = true;
  }
  if (invalid) return 255;
  if (hasBoth || (hasRead && hasWrite)) return 0;
  if (hasRead) return 1;
  if (hasWrite) return 2;
  if (hasIndexer) return 3;
  return 255;
}

export const GeneratedActionBuilders = {
  /** Publish a NIP-25 reaction to a target event. */
  react(
    correlationId: string,
    targetEventId: string,
    reaction: string,
    targetAuthorPubkey: string | null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const targetEventIdOffset = fbb.createString(targetEventId);
    const reactionOffset = fbb.createString(reaction);
    const targetAuthorPubkeyOffset = targetAuthorPubkey === null ? 0 : fbb.createString(targetAuthorPubkey);
    fbb.startObject(4);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, targetEventIdOffset, 0); // slot 1: targetEventId
    fbb.addFieldOffset(2, reactionOffset, 0); // slot 2: reaction
    if (targetAuthorPubkeyOffset !== 0) fbb.addFieldOffset(3, targetAuthorPubkeyOffset, 0); // slot 3: targetAuthorPubkey
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N25R");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip25.react", payload);
  },

  /** Retract a previously-published NIP-25 reaction. */
  unreact(
    correlationId: string,
    reactionEventId: string,
    reason: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const reactionEventIdOffset = fbb.createString(reactionEventId);
    const reasonOffset = fbb.createString(reason);
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, reactionEventIdOffset, 0); // slot 1: reactionEventId
    fbb.addFieldOffset(2, reasonOffset, 0); // slot 2: reason
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N25U");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip25.unreact", payload);
  },

  /** Publish a NIP-18 repost wrapper for a target event. */
  repost(
    correlationId: string,
    targetEventId: string,
    targetKind: number,
    targetAuthorPubkey: string | null,
    relayHint: string | null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const targetEventIdOffset = fbb.createString(targetEventId);
    const targetAuthorPubkeyOffset = targetAuthorPubkey === null ? 0 : fbb.createString(targetAuthorPubkey);
    const relayHintOffset = relayHint === null ? 0 : fbb.createString(relayHint);
    fbb.startObject(5);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, targetEventIdOffset, 0); // slot 1: targetEventId
    fbb.addFieldInt32(2, targetKind, 0); // slot 2: targetKind
    if (targetAuthorPubkeyOffset !== 0) fbb.addFieldOffset(3, targetAuthorPubkeyOffset, 0); // slot 3: targetAuthorPubkey
    if (relayHintOffset !== 0) fbb.addFieldOffset(4, relayHintOffset, 0); // slot 4: relayHint
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N18R");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip18.repost", payload);
  },

  /** Publish a NIP-18 quote repost note for a target event. */
  quoteRepost(
    correlationId: string,
    targetEventId: string,
    targetKind: number,
    targetAuthorPubkey: string | null,
    relayHint: string | null,
    content: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const targetEventIdOffset = fbb.createString(targetEventId);
    const targetAuthorPubkeyOffset = targetAuthorPubkey === null ? 0 : fbb.createString(targetAuthorPubkey);
    const relayHintOffset = relayHint === null ? 0 : fbb.createString(relayHint);
    const contentOffset = fbb.createString(content);
    fbb.startObject(6);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, targetEventIdOffset, 0); // slot 1: targetEventId
    fbb.addFieldInt32(2, targetKind, 0); // slot 2: targetKind
    if (targetAuthorPubkeyOffset !== 0) fbb.addFieldOffset(3, targetAuthorPubkeyOffset, 0); // slot 3: targetAuthorPubkey
    if (relayHintOffset !== 0) fbb.addFieldOffset(4, relayHintOffset, 0); // slot 4: relayHint
    fbb.addFieldOffset(5, contentOffset, 0); // slot 5: content
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N18Q");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip18.quote_repost", payload);
  },

  /** Follow a single pubkey (NIP-02 contact-list add). */
  follow(
    correlationId: string,
    pubkey: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const pubkeyOffset = fbb.createString(pubkey);
    fbb.startObject(2);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, pubkeyOffset, 0); // slot 1: pubkey
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NF2A");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.follow", payload);
  },

  /** Unfollow a single pubkey (NIP-02 contact-list remove). */
  unfollow(
    correlationId: string,
    pubkey: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const pubkeyOffset = fbb.createString(pubkey);
    fbb.startObject(2);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, pubkeyOffset, 0); // slot 1: pubkey
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NF2A");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.unfollow", payload);
  },

  /** Follow many pubkeys in one race-free read-modify-write cycle (NIP-02). */
  followMany(
    correlationId: string,
    pubkeys: string[] | null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const pubkeysOffset =
      pubkeys === null || pubkeys.length === 0 ? 0 : stringVector(fbb, pubkeys);
    fbb.startObject(2);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    if (pubkeysOffset !== 0) fbb.addFieldOffset(1, pubkeysOffset, 0); // slot 1: pubkeys
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NFMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.follow_many", payload);
  },

  /** Add one item to the active account's NIP-51 bookmark list. */
  addBookmark(
    correlationId: string,
    accountPubkey: string,
    itemKind: number,
    value: string,
    relay: string | null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const accountPubkeyOffset = fbb.createString(accountPubkey);
    const valueOffset = fbb.createString(value);
    const relayOffset = relay === null ? 0 : fbb.createString(relay);
    fbb.startObject(3);
    fbb.addFieldInt8(0, itemKind, 0); // slot 0: kind
    fbb.addFieldOffset(1, valueOffset, 0); // slot 1: value
    if (relayOffset !== 0) fbb.addFieldOffset(2, relayOffset, 0); // slot 2: relay
    const itemRoot = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, accountPubkeyOffset, 0); // slot 1: account_pubkey
    fbb.addFieldOffset(2, itemRoot, 0); // slot 2: item
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N51B");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip51.add_bookmark", payload);
  },

  /** Remove one item from the active account's NIP-51 bookmark list. */
  removeBookmark(
    correlationId: string,
    accountPubkey: string,
    itemKind: number,
    value: string,
    relay: string | null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const accountPubkeyOffset = fbb.createString(accountPubkey);
    const valueOffset = fbb.createString(value);
    const relayOffset = relay === null ? 0 : fbb.createString(relay);
    fbb.startObject(3);
    fbb.addFieldInt8(0, itemKind, 0); // slot 0: kind
    fbb.addFieldOffset(1, valueOffset, 0); // slot 1: value
    if (relayOffset !== 0) fbb.addFieldOffset(2, relayOffset, 0); // slot 2: relay
    const itemRoot = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, accountPubkeyOffset, 0); // slot 1: account_pubkey
    fbb.addFieldOffset(2, itemRoot, 0); // slot 2: item
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N51B");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip51.remove_bookmark", payload);
  },

  /** Add a relay URL to the NIP-51 blocked-relay list. */
  blockRelay(
    correlationId: string,
    url: string,
    accountPubkey: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const urlOffset = fbb.createString(url);
    const accountPubkeyOffset = fbb.createString(accountPubkey);
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, urlOffset, 0); // slot 1: url
    fbb.addFieldOffset(2, accountPubkeyOffset, 0); // slot 2: accountPubkey
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NBLK");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip51.block_relay", payload);
  },

  /** Remove a relay URL from the NIP-51 blocked-relay list. */
  unblockRelay(
    correlationId: string,
    url: string,
    accountPubkey: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const urlOffset = fbb.createString(url);
    const accountPubkeyOffset = fbb.createString(accountPubkey);
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, urlOffset, 0); // slot 1: url
    fbb.addFieldOffset(2, accountPubkeyOffset, 0); // slot 2: accountPubkey
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NUBL");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip51.unblock_relay", payload);
  },

  /** Publish a NIP-17 DM relay list (kind:10050). */
  publishDmRelayList(
    correlationId: string,
    relays: string[],
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const relaysOffset = stringVector(fbb, relays);
    fbb.startObject(2);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, relaysOffset, 0); // slot 1: relays
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N17R");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip17.publish_relay_list", payload);
  },

  /** Hydrate a DM peer's NIP-17 relay list (kind:10050). */
  hydrateDmPeerRelayList(
    correlationId: string,
    peerPubkey: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const peerPubkeyOffset = fbb.createString(peerPubkey);
    fbb.startObject(2);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, peerPubkeyOffset, 0); // slot 1: peerPubkey
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N17H");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip17.hydrate_peer_relay_list", payload);
  },

  /** Publish a NIP-65 relay-list metadata event (kind:10002). */
  publishRelayList(
    correlationId: string,
    relays: Array<{ url: string; role: string }>,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const relaysOffset = (() => {
      const entryOffsets: number[] = relays.map((r) => {
        const urlOff = fbb.createString(r.url);
        fbb.startObject(2);
        fbb.addFieldOffset(0, urlOff, 0); // RelayListEntry slot 0: url
        fbb.addFieldInt8(1, relayMarkerByte(r.role), 0); // RelayListEntry slot 1: marker
        return fbb.endObject();
      });
      fbb.startVector(4, entryOffsets.length, 4);
      for (let i = entryOffsets.length - 1; i >= 0; i--) fbb.addOffset(entryOffsets[i]!);
      return fbb.endVector();
    })();
    fbb.startObject(2);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, relaysOffset, 0); // slot 1: relays
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N65P");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip65.publish_relay_list", payload);
  },

  /** Connect a NIP-47 Nostr Wallet Connect URI. */
  walletConnect(
    correlationId: string,
    uri: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const uriOffset = fbb.createString(uri);
    fbb.startObject(2);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, uriOffset, 0); // slot 1: uri
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N47C");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.wallet.connect", payload);
  },

  /** Disconnect the current NIP-47 wallet (no payload data beyond schema_version). */
  walletDisconnect(
    correlationId: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    fbb.startObject(1);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N47D");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.wallet.disconnect", payload);
  },

  /** Pay a Lightning invoice via the NIP-47 wallet. */
  walletPayInvoice(
    correlationId: string,
    bolt11: string,
    amountMsats: bigint | null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const bolt11Offset = fbb.createString(bolt11);
    fbb.startObject(4);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, bolt11Offset, 0); // slot 1: bolt11
    if (amountMsats !== null) {
      fbb.addFieldInt64(2, amountMsats, BigInt(0)); // slot 2: amountMsats
      fbb.addFieldInt8(3, 1, 0); // slot 3: hasAmountMsats (bool)
    }
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N47P");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.wallet.pay_invoice", payload);
  },

  /** Send a NIP-17 gift-wrapped direct message to a recipient. */
  sendDm(
    correlationId: string,
    recipientPubkey: string,
    content: string,
    replyTo: string | null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const recipientPubkeyOffset = fbb.createString(recipientPubkey);
    const contentOffset = fbb.createString(content);
    const replyToOffset = replyTo === null ? 0 : fbb.createString(replyTo);
    fbb.startObject(4);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, recipientPubkeyOffset, 0); // slot 1: recipientPubkey
    fbb.addFieldOffset(2, contentOffset, 0); // slot 2: content
    if (replyToOffset !== 0) fbb.addFieldOffset(3, replyToOffset, 0); // slot 3: replyTo
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N17S");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip17.send", payload);
  },

  /** Publish a NIP-57 zap request for a recipient (optionally a target event). */
  zap(
    correlationId: string,
    recipientPubkey: string,
    amountMsats: bigint,
    lnurl: string | null,
    relays: string[],
    targetEventId: string | null,
    comment: string | null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const recipientPubkeyOffset = fbb.createString(recipientPubkey);
    const lnurlOffset = lnurl === null ? 0 : fbb.createString(lnurl);
    const relaysOffset = stringVector(fbb, relays);
    const targetEventIdOffset = targetEventId === null ? 0 : fbb.createString(targetEventId);
    const commentOffset = comment === null ? 0 : fbb.createString(comment);
    fbb.startObject(7);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, recipientPubkeyOffset, 0); // slot 1: recipientPubkey
    fbb.addFieldInt64(2, amountMsats, BigInt(0)); // slot 2: amountMsats
    if (lnurlOffset !== 0) fbb.addFieldOffset(3, lnurlOffset, 0); // slot 3: lnurl
    fbb.addFieldOffset(4, relaysOffset, 0); // slot 4: relays
    if (targetEventIdOffset !== 0) fbb.addFieldOffset(5, targetEventIdOffset, 0); // slot 5: targetEventId
    if (commentOffset !== 0) fbb.addFieldOffset(6, commentOffset, 0); // slot 6: comment
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "N57Z");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.nip57.zap", payload);
  },

  /** Sign-and-publish an arbitrary event kind (generic publish path; NIP-65 outbox or explicit relays). */
  publishRaw(
    correlationId: string,
    kind: number,
    tags: string[][],
    content: string,
    relays: string[] | null = null,
    signerPubkey: string | null = null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const tagRowOffsets = tags.map((row) => {
      const valuesVec = stringVector(fbb, row);
      fbb.startObject(1);
      fbb.addFieldOffset(0, valuesVec, 0); // slot 0: values
      return fbb.endObject();
    });
    fbb.startVector(4, tagRowOffsets.length, 4);
    for (let i = tagRowOffsets.length - 1; i >= 0; i--) fbb.addOffset(tagRowOffsets[i]!);
    const tagsVec = fbb.endVector();
    const contentOffset = fbb.createString(content);
    const signerPubkeyOffset = signerPubkey === null ? 0 : fbb.createString(signerPubkey);
    const targetRelays = relays ?? [];
    const explicit = targetRelays.length > 0;
    const targetRelaysVec = stringVector(fbb, targetRelays);
    fbb.startObject(2);
    fbb.addFieldInt8(0, explicit ? 1 : 0, 0); // slot 0: explicit
    fbb.addFieldOffset(1, targetRelaysVec, 0); // slot 1: relays
    const targetOffset = fbb.endObject();
    fbb.startObject(5);
    fbb.addFieldInt32(0, kind, 0); // slot 0: kind
    fbb.addFieldOffset(1, tagsVec, 0); // slot 1: tags
    fbb.addFieldOffset(2, contentOffset, 0); // slot 2: content
    fbb.addFieldOffset(3, targetOffset, 0); // slot 3: target
    if (signerPubkeyOffset !== 0) fbb.addFieldOffset(4, signerPubkeyOffset, 0); // slot 4: signer_pubkey
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 3, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NPUB");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.publish", payload);
  },

  /** Sign-and-publish a kind:1 reply; Rust derives NIP-10 tags from the stored parent event. */
  publishReply(
    correlationId: string,
    content: string,
    replyToEventId: string,
    relays: string[] | null = null,
    signerPubkey: string | null = null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const contentOffset = fbb.createString(content);
    const replyToEventIdOffset = fbb.createString(replyToEventId);
    const signerPubkeyOffset = signerPubkey === null ? 0 : fbb.createString(signerPubkey);
    const targetRelays = relays ?? [];
    const explicit = targetRelays.length > 0;
    const targetRelaysVec = stringVector(fbb, targetRelays);
    fbb.startObject(2);
    fbb.addFieldInt8(0, explicit ? 1 : 0, 0); // slot 0: explicit
    fbb.addFieldOffset(1, targetRelaysVec, 0); // slot 1: relays
    const targetOffset = fbb.endObject();
    fbb.startObject(4);
    fbb.addFieldOffset(0, contentOffset, 0); // slot 0: content
    fbb.addFieldOffset(1, replyToEventIdOffset, 0); // slot 1: reply_to_event_id
    fbb.addFieldOffset(2, targetOffset, 0); // slot 2: target
    if (signerPubkeyOffset !== 0) fbb.addFieldOffset(3, signerPubkeyOffset, 0); // slot 3: signer_pubkey
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 4, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NPUB");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.publish", payload);
  },

  /** Sign-and-publish a kind:0 profile metadata event for the active account. */
  publishProfile(
    correlationId: string,
    fields: Array<[string, string]>,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const profileFieldOffsets = fields.map(([key, value]) => {
      const keyOffset = fbb.createString(key);
      const valueOffset = fbb.createString(value);
      fbb.startObject(2);
      fbb.addFieldOffset(0, keyOffset, 0); // slot 0: key
      fbb.addFieldOffset(1, valueOffset, 0); // slot 1: value
      return fbb.endObject();
    });
    fbb.startVector(4, profileFieldOffsets.length, 4);
    for (let i = profileFieldOffsets.length - 1; i >= 0; i--) fbb.addOffset(profileFieldOffsets[i]!);
    const fieldsVec = fbb.endVector();
    fbb.startObject(1);
    fbb.addFieldOffset(0, fieldsVec, 0); // slot 0: fields
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 2, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NPUB");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.publish", payload);
  },

  /** Publish (or rotate) the local MLS key-package (kind:30443) to relays. */
  marmotPublishKeyPackage(
    correlationId: string,
    relays: string[] = [],
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const relaysVec = stringVector(fbb, relays);
    fbb.startObject(1);
    fbb.addFieldOffset(0, relaysVec, 0); // slot 0: relays
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 1, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NMMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.marmot", payload);
  },

  /** Create a new MLS group and optionally invite peers. */
  marmotCreateGroup(
    correlationId: string,
    name: string,
    description: string = "",
    inviteeText: string | null = null,
    inviteeNpubs: string[] | null = null,
    signedKeyPackageEventsJson: string[] = [],
    relays: string[] = [],
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const relaysVec = stringVector(fbb, relays);
    const jsonVec = stringVector(fbb, signedKeyPackageEventsJson);
    // inviteeNpubs: null → absent (None); non-null → present vector (even if empty)
    const npubsVec = inviteeNpubs === null ? 0 : stringVector(fbb, inviteeNpubs);
    const inviteeTextOffset = inviteeText === null ? 0 : fbb.createString(inviteeText);
    const descOffset = description === "" ? 0 : fbb.createString(description);
    const nameOffset = fbb.createString(name);
    fbb.startObject(6);
    fbb.addFieldOffset(0, nameOffset, 0); // slot 0: name (required)
    if (descOffset !== 0) fbb.addFieldOffset(1, descOffset, 0); // slot 1: description
    if (inviteeTextOffset !== 0) fbb.addFieldOffset(2, inviteeTextOffset, 0); // slot 2: invitee_text
    if (npubsVec !== 0) fbb.addFieldOffset(3, npubsVec, 0); // slot 3: invitee_npubs
    fbb.addFieldOffset(4, jsonVec, 0); // slot 4: signed_key_package_events_json
    fbb.addFieldOffset(5, relaysVec, 0); // slot 5: relays
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 2, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NMMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.marmot", payload);
  },

  /** Invite one or more peers to an existing MLS group. */
  marmotInvite(
    correlationId: string,
    groupIdHex: string,
    inviteeText: string | null = null,
    inviteeNpubs: string[] | null = null,
    signedKeyPackageEventsJson: string[] = [],
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const jsonVec = stringVector(fbb, signedKeyPackageEventsJson);
    const npubsVec = inviteeNpubs === null ? 0 : stringVector(fbb, inviteeNpubs);
    const inviteeTextOffset = inviteeText === null ? 0 : fbb.createString(inviteeText);
    const gidOffset = fbb.createString(groupIdHex);
    fbb.startObject(4);
    fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)
    if (inviteeTextOffset !== 0) fbb.addFieldOffset(1, inviteeTextOffset, 0); // slot 1: invitee_text
    if (npubsVec !== 0) fbb.addFieldOffset(2, npubsVec, 0); // slot 2: invitee_npubs
    fbb.addFieldOffset(3, jsonVec, 0); // slot 3: signed_key_package_events_json
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 3, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NMMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.marmot", payload);
  },

  /** Send a kind:14 NIP-44 MLS group message. */
  marmotSend(
    correlationId: string,
    groupIdHex: string,
    text: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const textOffset = fbb.createString(text);
    const gidOffset = fbb.createString(groupIdHex);
    fbb.startObject(2);
    fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)
    fbb.addFieldOffset(1, textOffset, 0); // slot 1: text (required)
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 4, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NMMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.marmot", payload);
  },

  /** Self-remove from a MLS group (SelfRemove proposal + commit). */
  marmotLeave(
    correlationId: string,
    groupIdHex: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const gidOffset = fbb.createString(groupIdHex);
    fbb.startObject(1);
    fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 5, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NMMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.marmot", payload);
  },

  /** Remove other members from a MLS group (Remove proposal + commit). */
  marmotRemove(
    correlationId: string,
    groupIdHex: string,
    memberNpubs: string[] = [],
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const npubsVec = stringVector(fbb, memberNpubs);
    const gidOffset = fbb.createString(groupIdHex);
    fbb.startObject(2);
    fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)
    fbb.addFieldOffset(1, npubsVec, 0); // slot 1: member_npubs
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 6, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NMMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.marmot", payload);
  },

  /** Accept a pending MLS Welcome (by gift-wrap event id hex). */
  marmotAcceptWelcome(
    correlationId: string,
    welcomeIdHex: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const widOffset = fbb.createString(welcomeIdHex);
    fbb.startObject(1);
    fbb.addFieldOffset(0, widOffset, 0); // slot 0: welcome_id_hex (required)
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 7, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NMMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.marmot", payload);
  },

  /** Decline a pending MLS Welcome. */
  marmotDeclineWelcome(
    correlationId: string,
    welcomeIdHex: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const widOffset = fbb.createString(welcomeIdHex);
    fbb.startObject(1);
    fbb.addFieldOffset(0, widOffset, 0); // slot 0: welcome_id_hex (required)
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 8, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NMMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.marmot", payload);
  },

  /** Explicitly clear the pending-commit state for a MLS group. */
  marmotClearPending(
    correlationId: string,
    groupIdHex: string,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const gidOffset = fbb.createString(groupIdHex);
    fbb.startObject(1);
    fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)
    const bodyOffset = fbb.endObject();
    fbb.startObject(3);
    fbb.addFieldInt32(0, 1, 0); // slot 0: schema_version
    fbb.addFieldInt8(1, 9, 0); // slot 1: body_type
    fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "NMMA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "nmp.marmot", payload);
  },

};
