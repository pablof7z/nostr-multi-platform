/**
 * content-core (web) — the shared decode seam every web content component reads.
 *
 * `ContentTreeWire` is the kernel's NFCT projection (the `nmp-content` tokenizer
 * behind the kernel's content-parser seam), shipped as FlatBuffers bytes inside
 * the snapshot (`claimed_events.content_tree_bytes` / feed projections). This
 * module is the one place that turns those raw bytes into a decoded
 * `ContentTreeWire` root — every content component (content-view, content-minimal)
 * consumes the decoded tree, never raw bytes, so decoding lives here once.
 *
 * Re-exports the generated wire type so consumers import the tree type and the
 * decoder from a single content-core module, mirroring the native content-core
 * `ContentTreeWire` + renderer split. Pure; never fetches or mocks (D6/D7).
 */
import * as flatbuffers from "flatbuffers";
import { ContentTreeWire } from "@nmp/wire-ts/nmp/content/content-tree-wire";
import { WireNodeKind } from "@nmp/wire-ts/nmp/content/wire-node-kind";

export { ContentTreeWire, WireNodeKind };
export type { WireNode } from "@nmp/wire-ts/nmp/content/wire-node";

/**
 * Decode NFCT bytes into a `ContentTreeWire` root, or `undefined` when the bytes
 * are empty or lack the `NFCT` file identifier (keep-last-good / honest-empty
 * per D6 — callers fall back to the raw content string).
 */
export function decodeContentTree(bytes: Uint8Array | null | undefined): ContentTreeWire | undefined {
  if (!bytes || bytes.length === 0) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!ContentTreeWire.bufferHasIdentifier(bb)) return undefined;
    return ContentTreeWire.getRootAsContentTreeWire(bb);
  } catch {
    return undefined;
  }
}

/** True when a decoded tree is renderable from the tree path: non-empty AND free
 *  of `Placeholder` nodes (an unresolved `nostr:` URI becomes a placeholder).
 *  The honesty gate the gallery uses to refuse the raw-string fallback. */
export function isTreeRenderable(tree: ContentTreeWire | undefined): boolean {
  if (!tree || tree.rootsLength() === 0) return false;
  for (let i = 0; i < tree.nodesLength(); i += 1) {
    const node = tree.nodes(i);
    if (node && node.kind() === WireNodeKind.Placeholder) return false;
  }
  return true;
}
