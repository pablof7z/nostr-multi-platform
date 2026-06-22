import { type JSX } from "solid-js";

import { WireNodeKind } from "./nmp/generated/nmp/content/wire-node-kind";
import { type ClaimedEventWire, type RelayStatusRow, tagValue } from "./nmp/profileHost";
import type {
  ArticleProjection,
  EmbeddedEventModel,
  EmbedAuthor,
  HighlightProjection,
} from "@nmp/components-web/src/content-kind-registry/NostrKindRegistry";
import type { ProfileWire } from "@nmp/components-web/src/user-avatar/ProfileWire";

// #1767 — pure projections of the kernel-RESOLVED embed envelope. The standalone
// embed-article / embed-highlight showcase sections render their card fields
// from the resolved per-kind projection (NOT re-parsed NIP-23/NIP-84 tags); these
// narrow the projection union to the variant payload. Pure — App.tsx wraps them
// in a `createMemo` so reactivity stays at the component call site.

/** The `article` variant payload of a resolved embed, else undefined. */
export function articleProjectionOf(
  embed: EmbeddedEventModel | undefined,
): ArticleProjection | undefined {
  if (!embed) return undefined;
  return embed.projection.variant === "article" ? embed.projection.data : undefined;
}

/** The `highlight` variant payload of a resolved embed, else undefined. */
export function highlightProjectionOf(
  embed: EmbeddedEventModel | undefined,
): HighlightProjection | undefined {
  if (!embed) return undefined;
  return embed.projection.variant === "highlight" ? embed.projection.data : undefined;
}

/** Host-resolved author byline from a resolved profile. The kernel projection
 *  carries no author by design; the displaying host resolves it from the
 *  `refs.profile` store and threads it into the registry card. */
export function bylineOf(profile: ProfileWire | undefined): EmbedAuthor | undefined {
  return profile ? { name: profile.displayName, picture: profile.pictureUrl } : undefined;
}

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
