/**
 * NostrMediaGrid — adaptive 1–4 image grid for inline media attached to a note.
 *
 * Pure renderer (D7): takes the list of media URLs the host extracted from the
 * content tree (NFCT `Media` node) or `imeta` tags. Layout is count-driven:
 * 1 = full-width 16:9, 2 = side-by-side, 3 = one large + two stacked, 4+ = 2×2
 * with a `+N` overlay on the last cell — identical to the SwiftUI/Compose
 * variants. The host owns fetching/caching; this component only lays out the
 * `<img>` cells it is given.
 */
import type { JSX } from "solid-js";
import { For, Show } from "solid-js";

export function NostrMediaGrid(props: { urls: string[] }): JSX.Element {
  const shown = () => props.urls.slice(0, 4);
  const overflow = () => Math.max(0, props.urls.length - 4);
  return (
    <Show when={props.urls.length > 0}>
      <div class="nostr-media-grid" data-count={Math.min(4, props.urls.length)}>
        <For each={shown()}>
          {(url, i) => (
            <a
              class="nostr-media-grid__cell"
              href={url}
              rel="noopener noreferrer"
              target="_blank"
            >
              <img src={url} alt="" loading="lazy" />
              <Show when={i() === 3 && overflow() > 0}>
                <span class="nostr-media-grid__overflow">+{overflow()}</span>
              </Show>
            </a>
          )}
        </For>
      </div>
    </Show>
  );
}
