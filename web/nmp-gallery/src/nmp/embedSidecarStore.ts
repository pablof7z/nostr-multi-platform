import * as flatbuffers from "flatbuffers";

import { ContentTreeWire as FbContentTreeWire } from "./generated/nmp/content/content-tree-wire";
import type { WireNode as FbWireNode } from "./generated/nmp/content/wire-node";
import { WireNodeKind } from "./generated/nmp/content/wire-node-kind";
import { RefEventEnvelopes } from "./generated/nmp/embed/ref-event-envelopes";
import { EmbedProjectionKind } from "./generated/nmp/embed/embed-projection-kind";
import type { EmbedKindProjection as FbEmbedKindProjection } from "./generated/nmp/embed/embed-kind-projection";
import type { EmbeddedEventEnvelope as FbEmbeddedEventEnvelope } from "./generated/nmp/embed/embedded-event-envelope";
import type { TagRow } from "./generated/nmp/embed/tag-row";
import type {
  ContentTreeWire,
  EmbeddedEventModel,
  EmbedKindProjection,
  WireNode,
} from "@nmp/components-web/src/content-kind-registry/NostrKindRegistry";

export const EMBED_SIDECAR_KEY = "refs.event.envelopes";
export const NEMB_FILE_IDENTIFIER = "NEMB";

const utf8Decoder = new TextDecoder();

export function decodeEmbedSidecar(bytes: Uint8Array): Map<string, EmbeddedEventModel> | undefined {
  if (bytes.length < 8) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!RefEventEnvelopes.bufferHasIdentifier(bb)) return undefined;
    const root = RefEventEnvelopes.getRootAsRefEventEnvelopes(bb);
    const out = new Map<string, EmbeddedEventModel>();
    for (let i = 0; i < root.entriesLength(); i += 1) {
      const env = root.entries(i);
      const decoded = env ? decodeEnvelope(env) : undefined;
      if (decoded) out.set(decoded.primaryId, decoded);
    }
    return out;
  } catch {
    return undefined;
  }
}

function decodeEnvelope(env: FbEmbeddedEventEnvelope): EmbeddedEventModel | undefined {
  const primaryId = asString(env.primaryId());
  const projection = env.projection();
  if (!primaryId || !projection) return undefined;
  const decodedProjection = decodeProjection(projection);
  if (!decodedProjection) return undefined;
  return {
    uri: asString(env.uri()) ?? "",
    primaryId,
    projection: decodedProjection,
    collapsed: env.collapsed(),
    collapseReason: opt(env.hasCollapseReason(), env.collapseReason()),
  };
}

function decodeProjection(p: FbEmbedKindProjection): EmbedKindProjection | undefined {
  switch (p.kind()) {
    case EmbedProjectionKind.ShortNote: {
      const n = p.shortNote();
      if (!n) return undefined;
      return {
        variant: "shortNote",
        data: {
          id: asString(n.id()) ?? "",
          authorPubkey: asString(n.authorPubkey()) ?? "",
          authorDisplayName: opt(n.hasAuthorDisplayName(), n.authorDisplayName()),
          authorPictureUrl: opt(n.hasAuthorPictureUrl(), n.authorPictureUrl()),
          createdAt: Number(n.createdAt()),
          contentTree: decodeContentTree(n.contentTreeArray()),
          mediaUrls: stringVector(n.mediaUrlsLength(), (i) => n.mediaUrls(i)),
        },
      };
    }
    case EmbedProjectionKind.Article: {
      const a = p.article();
      if (!a) return undefined;
      return {
        variant: "article",
        data: {
          id: asString(a.id()) ?? "",
          authorPubkey: asString(a.authorPubkey()) ?? "",
          authorDisplayName: opt(a.hasAuthorDisplayName(), a.authorDisplayName()),
          authorPictureUrl: opt(a.hasAuthorPictureUrl(), a.authorPictureUrl()),
          createdAt: Number(a.createdAt()),
          title: opt(a.hasTitle(), a.title()),
          summary: opt(a.hasSummary(), a.summary()),
          heroImageUrl: opt(a.hasHeroImageUrl(), a.heroImageUrl()),
          dTag: asString(a.dTag()) ?? "",
        },
      };
    }
    case EmbedProjectionKind.Highlight: {
      const h = p.highlight();
      if (!h) return undefined;
      return {
        variant: "highlight",
        data: {
          id: asString(h.id()) ?? "",
          authorPubkey: asString(h.authorPubkey()) ?? "",
          authorDisplayName: opt(h.hasAuthorDisplayName(), h.authorDisplayName()),
          createdAt: Number(h.createdAt()),
          highlightedText: asString(h.highlightedText()) ?? "",
          sourceEventId: opt(h.hasSourceEventId(), h.sourceEventId()),
          sourceEventAddr: opt(h.hasSourceEventAddr(), h.sourceEventAddr()),
          sourceUrl: opt(h.hasSourceUrl(), h.sourceUrl()),
          context: opt(h.hasContext(), h.context()),
        },
      };
    }
    case EmbedProjectionKind.Profile: {
      const pr = p.profile();
      if (!pr) return undefined;
      return {
        variant: "profile",
        data: {
          pubkey: asString(pr.pubkey()) ?? "",
          displayName: opt(pr.hasDisplayName(), pr.displayName()),
          pictureUrl: opt(pr.hasPictureUrl(), pr.pictureUrl()),
          about: opt(pr.hasAbout(), pr.about()),
          nip05: opt(pr.hasNip05(), pr.nip05()),
          lud16: opt(pr.hasLud16(), pr.lud16()),
          bannerUrl: opt(pr.hasBannerUrl(), pr.bannerUrl()),
        },
      };
    }
    case EmbedProjectionKind.Unknown: {
      const u = p.unknown();
      if (!u) return undefined;
      return {
        variant: "unknown",
        data: {
          kind: u.kind(),
          authorPubkey: asString(u.authorPubkey()) ?? "",
          authorDisplayName: opt(u.hasAuthorDisplayName(), u.authorDisplayName()),
          authorPictureUrl: opt(u.hasAuthorPictureUrl(), u.authorPictureUrl()),
          createdAt: Number(u.createdAt()),
          content: asString(u.content()) ?? "",
          tags: tagRows(u),
          altText: opt(u.hasAltText(), u.altText()),
        },
      };
    }
    default:
      return undefined;
  }
}

function decodeContentTree(bytes: Uint8Array | null): ContentTreeWire {
  if (!bytes || bytes.length < 8) return { nodes: [], roots: [] };
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!FbContentTreeWire.bufferHasIdentifier(bb)) return { nodes: [], roots: [] };
    const root = FbContentTreeWire.getRootAsContentTreeWire(bb);
    const nodes: WireNode[] = [];
    for (let i = 0; i < root.nodesLength(); i += 1) {
      const node = root.nodes(i);
      if (node) nodes.push(decodeNode(node));
    }
    const roots: number[] = [];
    for (let i = 0; i < root.rootsLength(); i += 1) roots.push(root.roots(i) ?? 0);
    return { nodes, roots };
  } catch {
    return { nodes: [], roots: [] };
  }
}

function decodeNode(node: FbWireNode): WireNode {
  const kind = nodeKind(node.kind());
  const out: WireNode = { kind };
  const text = asString(node.text());
  const tag = asString(node.tag());
  const url = asString(node.url()) ?? asString(node.href());
  if (text) out.text = text;
  if (tag) out.tag = tag;
  if (url) out.url = url;
  if (kind === "inline_code" && text) out.code = text;
  return out;
}

function nodeKind(kind: WireNodeKind): string {
  switch (kind) {
    case WireNodeKind.Hashtag:
      return "hashtag";
    case WireNodeKind.Url:
      return "url";
    case WireNodeKind.InlineCode:
      return "inline_code";
    case WireNodeKind.SoftBreak:
      return "soft_break";
    case WireNodeKind.HardBreak:
      return "hard_break";
    case WireNodeKind.Mention:
      return "mention";
    case WireNodeKind.EventRef:
      return "event_ref";
    case WireNodeKind.Image:
      return "image";
    case WireNodeKind.Media:
      return "media";
    case WireNodeKind.Placeholder:
      return "placeholder";
    default:
      return "text";
  }
}

function tagRows(u: { tagsLength(): number; tags(index: number): TagRow | null }): string[][] {
  const rows: string[][] = [];
  for (let i = 0; i < u.tagsLength(); i += 1) {
    const row = u.tags(i);
    if (!row) continue;
    rows.push(stringVector(row.valuesLength(), (j) => row.values(j)));
  }
  return rows;
}

function stringVector(length: number, value: (index: number) => string | Uint8Array | null): string[] {
  const out: string[] = [];
  for (let i = 0; i < length; i += 1) {
    const v = asString(value(i));
    if (v !== undefined) out.push(v);
  }
  return out;
}

function opt(present: boolean, value: string | Uint8Array | null): string | null {
  return present ? asString(value) ?? "" : null;
}

function asString(value: string | Uint8Array | null): string | undefined {
  if (typeof value === "string") return value;
  return value ? utf8Decoder.decode(value) : undefined;
}
