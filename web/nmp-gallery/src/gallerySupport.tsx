import { type JSX } from "solid-js";

import type { EmbeddedEventModel } from "@nmp/components";
import { WireNodeKind } from "@nmp/wire-ts/nmp/content/wire-node-kind";
import { type ClaimedEventWire, type RelayStatusRow, tagValue } from "./nmp/profileHost";

export function StatusBar(props: {
  status: unknown;
  relays: RelayStatusRow[];
  anyRelayConnected: boolean;
  resolvedCount: number;
}): JSX.Element {
  return (
    <div class="status-bar" data-testid="status-bar">
      <Pill
        label="kernel"
        value={String(typeof props.status === "string" ? props.status : "degraded")}
        ok={props.status === "running"}
      />
      <Pill
        label="relays"
        value={`${props.relays.filter((r) => r.connection.toLowerCase() === "connected").length}/${props.relays.length} connected`}
        ok={props.anyRelayConnected}
      />
      <Pill label="profiles resolved" value={String(props.resolvedCount)} ok={props.resolvedCount > 0} />
    </div>
  );
}

export function Section(props: {
  id: string;
  title: string;
  desc: string;
  children: JSX.Element;
}): JSX.Element {
  return (
    <section class="component-section" id={props.id}>
      <h2>{props.title}</h2>
      <p class="component-desc">{props.desc}</p>
      <div class="component-stage" data-component={props.title}>
        {props.children}
      </div>
    </section>
  );
}

export function Resolving(): JSX.Element {
  return (
    <div class="resolving" data-testid="resolving">
      <span class="resolving-dot" /> resolving from relays...
    </div>
  );
}

export function contentReady(ev: ClaimedEventWire | undefined): ClaimedEventWire | undefined {
  const tree = ev?.contentTree;
  if (!ev || !tree || tree.rootsLength() === 0) return undefined;
  for (let i = 0; i < tree.nodesLength(); i += 1) {
    if (tree.nodes(i)?.kind() === WireNodeKind.Placeholder) return undefined;
  }
  return ev;
}

export function toEmbedded(
  ev: ClaimedEventWire,
  author: { displayName?: string; pictureUrl?: string } | undefined,
): EmbeddedEventModel {
  return {
    kind: ev.kind,
    content: ev.content,
    createdAt: ev.createdAt,
    tags: ev.tags,
    authorName: author?.displayName,
    authorPicture: author?.pictureUrl,
  };
}

export function collectMediaUrls(ev: ClaimedEventWire | undefined): string[] {
  const out: string[] = [];
  const tree = ev?.contentTree;
  if (tree) {
    for (let i = 0; i < tree.nodesLength(); i += 1) {
      const node = tree.nodes(i);
      if (!node) continue;
      if (node.kind() === WireNodeKind.Image) {
        const url = node.url();
        if (url) out.push(url);
      } else if (node.kind() === WireNodeKind.Media) {
        for (let m = 0; m < node.mediaUrlsLength(); m += 1) {
          const url = node.mediaUrls(m) as string;
          if (url) out.push(url);
        }
      }
    }
  }
  const hero = ev ? tagValue(ev, "image") : undefined;
  if (hero && !out.includes(hero)) out.unshift(hero);
  return out;
}

function Pill(props: { label: string; value: string; ok: boolean }): JSX.Element {
  return (
    <span class="pill" classList={{ "pill--ok": props.ok }}>
      <span class="pill-label">{props.label}</span>
      <span class="pill-value">{props.value}</span>
    </span>
  );
}
