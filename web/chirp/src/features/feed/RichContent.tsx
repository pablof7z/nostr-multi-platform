import { For, Show } from "solid-js";
import "./rich-content.css";

type Segment =
  | { kind: "text"; value: string }
  | { kind: "url"; value: string; image: boolean }
  | { kind: "hashtag"; value: string }
  | { kind: "nostr"; value: string };

const TOKEN_RE = /(https?:\/\/[^\s<>()]+|nostr:[a-z0-9]+|n(?:pub|profile|event|ote|addr)1[023456789acdefghjklmnpqrstuvwxyz]+)/gi;
const IMAGE_RE = /\.(?:apng|avif|gif|jpe?g|png|webp)(?:[?#].*)?$/i;

function trimUrlToken(raw: string): { url: string; suffix: string } {
  let url = raw;
  let suffix = "";
  while (/[.,!?;:]$/.test(url)) {
    suffix = `${url.slice(-1)}${suffix}`;
    url = url.slice(0, -1);
  }
  return { url, suffix };
}

function pushTextSegments(segments: Segment[], text: string) {
  const parts = text.split(/(#[A-Za-z0-9_]{2,64})/g);
  for (const part of parts) {
    if (!part) continue;
    if (/^#[A-Za-z0-9_]{2,64}$/.test(part)) {
      segments.push({ kind: "hashtag", value: part });
    } else {
      segments.push({ kind: "text", value: part });
    }
  }
}

function parseContent(content: string): Segment[] {
  const segments: Segment[] = [];
  let cursor = 0;
  for (const match of content.matchAll(TOKEN_RE)) {
    const raw = match[0];
    const index = match.index ?? 0;
    if (index > cursor) {
      pushTextSegments(segments, content.slice(cursor, index));
    }
    if (raw.startsWith("http://") || raw.startsWith("https://")) {
      const { url, suffix } = trimUrlToken(raw);
      segments.push({ kind: "url", value: url, image: IMAGE_RE.test(url) });
      if (suffix) segments.push({ kind: "text", value: suffix });
    } else {
      segments.push({ kind: "nostr", value: raw });
    }
    cursor = index + raw.length;
  }
  if (cursor < content.length) {
    pushTextSegments(segments, content.slice(cursor));
  }
  return segments;
}

function compactUrl(value: string): string {
  try {
    const url = new URL(value);
    return `${url.hostname}${url.pathname === "/" ? "" : url.pathname}`;
  } catch {
    return value;
  }
}

function hashtagSearchHref(value: string): string {
  return `#search?q=${encodeURIComponent(value)}`;
}

export function RichContent(props: { content: string }) {
  const segments = () => parseContent(props.content);
  const imageSegments = () => segments().filter((segment) => segment.kind === "url" && segment.image);

  return (
    <div class="post-content" data-testid="post-content">
      <div class="post-content-text">
        <For each={segments()}>
          {(segment) => (
            <>
              <Show when={segment.kind === "text"}>{segment.value}</Show>
              <Show when={segment.kind === "url" && !segment.image}>
                <a
                  href={segment.value}
                  target="_blank"
                  rel="noreferrer"
                  data-testid="post-content-link"
                >
                  {compactUrl(segment.value)}
                </a>
              </Show>
              <Show when={segment.kind === "hashtag"}>
                <a
                  href={hashtagSearchHref(segment.value)}
                  class="post-hashtag"
                  data-testid="post-content-hashtag"
                  title={`Search ${segment.value}`}
                >
                  {segment.value}
                </a>
              </Show>
              <Show when={segment.kind === "nostr"}>
                <code class="post-nostr-ref" data-testid="post-content-nostr-ref">
                  {segment.value.slice(0, 18)}
                </code>
              </Show>
            </>
          )}
        </For>
      </div>

      <Show when={imageSegments().length > 0}>
        <div class="post-media-grid" data-testid="post-media-grid">
          <For each={imageSegments().slice(0, 2)}>
            {(segment) => (
              <a href={segment.value} target="_blank" rel="noreferrer" class="post-media-link">
                <img src={segment.value} alt="" loading="lazy" decoding="async" />
              </a>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
