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
import "./feed.css";

export function FeedPanel(props: { canPublish: boolean }) {
  const { state, profileHost } = createFeedStore();

  return (
    <NostrProfileHostProvider host={profileHost}>
      <div class="feed-panel" data-testid="feed-panel">
        {/* Compose box */}
        <Composer canPublish={props.canPublish} />

        {/* Timeline */}
        <div class="feed-timeline" data-testid="feed-timeline">
          <Show
            when={state.ready}
            fallback={
              <div
                class="feed-loading"
                data-testid="feed-loading"
              >
                Loading relay feed...
              </div>
            }
          >
            <Show
              when={state.rows.length > 0}
              fallback={
                <div
                  class="feed-empty"
                  data-testid="feed-empty"
                >
                  <strong>No notes yet</strong>
                  <span>Connect a signer with follows or use a relay bootstrap to hydrate the feed.</span>
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
