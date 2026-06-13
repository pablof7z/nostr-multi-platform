/**
 * NostrMediaGrid — adaptive 1–4 image/video media grid (web / SolidJS).
 *
 * Renders the `mediaUrls` array from an NFCT Media node into an adaptive
 * CSS grid: 1 = full-width 16:9, 2 = side-by-side, 3 = one large + two
 * stacked, 4 = 2×2. Five or more images show a 2×2 preview with a "+N"
 * overflow badge on the fourth cell.
 *
 * The component renders `<img>` elements with `loading="lazy"`. Swap the
 * inner `MediaCell` component to use your own image loading library
 * (e.g. a Solid signal-based cache) without touching the grid logic.
 *
 * Install: nmp add web/content-media-grid
 * Dependencies: solid-js
 */
import type { JSX } from "solid-js";
import { For, Show } from "solid-js";

// ── Props ─────────────────────────────────────────────────────────────────────

export type NostrMediaGridProps = {
  /**
   * Media URLs from the NFCT Media node (`node.mediaUrls`).
   * Each URL is rendered as a lazy-loaded image thumbnail.
   */
  urls: string[];
  /** Called when the user taps a media item. Receives the full URL and index. */
  onTap?: (url: string, index: number) => void;
};

// ── Public component ──────────────────────────────────────────────────────────

/**
 * Adaptive media grid — CSS `data-count` attribute drives layout via a single
 * CSS rule set: `[data-count="1"]`, `[data-count="2"]`, etc.
 *
 * Render nothing for empty URL arrays (honest-empty per D6).
 */
export function NostrMediaGrid(props: NostrMediaGridProps): JSX.Element {
  const urls = (): string[] => props.urls;
  const displayCount = (): number => Math.min(urls().length, 4);
  const overflow = (): number => Math.max(0, urls().length - 4);

  return (
    <Show when={urls().length > 0}>
      <div
        class="nostr-media-grid"
        data-count={displayCount()}
        role="group"
        aria-label={`${urls().length} media item${urls().length !== 1 ? "s" : ""}`}
      >
        <For each={urls().slice(0, 4)}>
          {(url, idx) => (
            <Show
              when={idx() === 3 && overflow() > 0}
              fallback={
                <MediaCell
                  url={url}
                  index={idx()}
                  onTap={props.onTap}
                />
              }
            >
              {/* 4th cell with overflow badge */}
              <button
                type="button"
                class="nostr-media-item nostr-media-overflow"
                onClick={() => props.onTap?.(url, idx())}
                aria-label={`Show ${overflow() + 1} more media items`}
              >
                <img
                  class="nostr-media-img"
                  src={url}
                  alt=""
                  loading="lazy"
                />
                <span class="nostr-media-overflow-badge">+{overflow() + 1}</span>
              </button>
            </Show>
          )}
        </For>
      </div>
    </Show>
  );
}

// ── Internal: single media cell ───────────────────────────────────────────────

function MediaCell(p: {
  url: string;
  index: number;
  onTap?: (url: string, index: number) => void;
}): JSX.Element {
  return (
    <button
      type="button"
      class="nostr-media-item"
      onClick={() => p.onTap?.(p.url, p.index)}
      aria-label="Open media"
    >
      <img
        class="nostr-media-img"
        src={p.url}
        alt=""
        loading="lazy"
      />
    </button>
  );
}
