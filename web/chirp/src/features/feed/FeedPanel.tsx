// FeedPanel.tsx — the home feed panel for Chirp Web (Item C).
//
// Composition root for the feed feature:
//   1. Calls `createFeedStore()` to get the reactive feed state + NostrProfileHost.
//   2. Mounts `<NostrProfileHostProvider>` so all avatar/name components can resolve.
//   3. Renders the `<Composer>` + a scrollable list of `<PostCard>` items.
//
// Zero Nostr protocol logic — decoding and dispatching are owned by feedDecoder.ts
// and feedStore.ts respectively. This file is pure presentation orchestration.

import { For, Show } from "solid-js";
import { NostrProfileHostProvider } from "@nmp/components-web/src/user-avatar/NostrProfileHost";
import { createFeedStore } from "../../nmp/feedStore";
import { PostCard } from "./PostCard";
import { Composer } from "./Composer";

export function FeedPanel() {
  const { state, profileHost } = createFeedStore();

  return (
    <NostrProfileHostProvider host={profileHost}>
      <div
        class="feed-panel"
        data-testid="feed-panel"
        style={{
          display: "flex",
          "flex-direction": "column",
          width: "100%",
          "max-width": "600px",
          margin: "0 auto",
          "border-left": "1px solid rgba(0,0,0,0.08)",
          "border-right": "1px solid rgba(0,0,0,0.08)",
          "min-height": "100vh",
          background: "white",
        }}
      >
        {/* Compose box */}
        <Composer />

        {/* Timeline */}
        <div class="feed-timeline" data-testid="feed-timeline">
          <Show
            when={state.ready}
            fallback={
              <div
                class="feed-loading"
                style={{
                  padding: "32px 16px",
                  "text-align": "center",
                  color: "#888",
                  "font-size": "0.9rem",
                }}
                data-testid="feed-loading"
              >
                Loading feed…
              </div>
            }
          >
            <Show
              when={state.rows.length > 0}
              fallback={
                <div
                  class="feed-empty"
                  style={{
                    padding: "32px 16px",
                    "text-align": "center",
                    color: "#888",
                    "font-size": "0.9rem",
                  }}
                  data-testid="feed-empty"
                >
                  No posts yet — follow some accounts to see their notes here.
                </div>
              }
            >
              <For each={state.rows}>
                {(row) => <PostCard row={row} />}
              </For>
            </Show>
          </Show>
        </div>
      </div>
    </NostrProfileHostProvider>
  );
}
