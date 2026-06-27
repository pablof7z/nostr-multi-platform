// FeedPanel.tsx — the home feed panel for Chirp Web (Item C).
//
// Composition root for the feed feature:
//   1. Calls `createFeedStore()` to get the reactive feed state + NostrProfileHost.
//   2. Mounts `<NostrProfileHostProvider>` so all avatar/name components can resolve.
//   3. Renders the `<Composer>` + a scrollable list of `<PostCard>` items.
//
// Zero Nostr protocol logic — decoding and dispatching are owned by feedDecoder.ts
// and feedStore.ts respectively. This file is pure presentation orchestration.

import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { NostrProfileHostProvider } from "@nmp/components-web/src/user-avatar/NostrProfileHost";
import { createFeedStore } from "../../nmp/feedStore";
import type { RuntimeProjection } from "../../nmp/runtimeProjection";
import { PostCard, type FeedSelection } from "./PostCard";
import { FeedDetailPanel } from "./FeedDetailPanel";
import { Composer } from "./Composer";
import "./feed.css";
import "./saved.css";

type FeedMode = "home" | "saved";

function modeFromHash(): FeedMode {
  return window.location.hash === "#saved" ? "saved" : "home";
}

export function FeedPanel(props: { canPublish: boolean; diagnostics?: RuntimeProjection }) {
  const { state, profileHost } = createFeedStore();
  const [selection, setSelection] = createSignal<FeedSelection | null>(null);
  const [quoteTarget, setQuoteTarget] = createSignal<FeedSelection["row"] | null>(null);
  const [mode, setMode] = createSignal<FeedMode>(modeFromHash());
  const bookmarkedIds = createMemo(() => new Set(props.diagnostics?.bookmarkedEventIds ?? []));
  const savedRows = createMemo(() => state.rows.filter((row) => bookmarkedIds().has(row.id)));
  const visibleRows = createMemo(() => (mode() === "saved" ? savedRows() : state.rows));
  const detailSelection = createMemo(() => {
    const current = selection();
    if (!current) return null;
    const latest = state.rows.find((row) => row.id === current.row.id);
    return latest ? { ...current, row: latest } : current;
  });
  const bookmarkedCount = () => props.diagnostics?.bookmarkedEventIds.length ?? 0;

  onMount(() => {
    const syncHash = () => setMode(modeFromHash());
    window.addEventListener("hashchange", syncHash);
    onCleanup(() => window.removeEventListener("hashchange", syncHash));
  });

  const selectMode = (next: FeedMode) => {
    setMode(next);
    const nextHash = next === "saved" ? "#saved" : "#feed";
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
  };

  const startQuote = (row: FeedSelection["row"]) => {
    setQuoteTarget(row);
    requestAnimationFrame(() => {
      document.querySelector<HTMLTextAreaElement>('[data-testid="compose-input"]')?.focus();
    });
  };

  return (
    <NostrProfileHostProvider host={profileHost}>
      <div class="feed-panel" data-testid="feed-panel" data-view={mode()}>
        {/* Compose box */}
        <Composer
          canPublish={props.canPublish}
          quoteTarget={quoteTarget()}
          onCancelQuote={() => setQuoteTarget(null)}
          onQuotePublished={() => setQuoteTarget(null)}
        />

        <div class="feed-viewbar" id="saved" aria-label="Feed views">
          <div class="feed-tabs" role="tablist" aria-label="Timeline views">
            <button
              type="button"
              role="tab"
              aria-selected={mode() === "home" ? "true" : "false"}
              data-active={mode() === "home" ? "true" : "false"}
              onClick={() => selectMode("home")}
            >
              Home
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={mode() === "saved" ? "true" : "false"}
              data-active={mode() === "saved" ? "true" : "false"}
              data-testid="saved-view-tab"
              onClick={() => selectMode("saved")}
            >
              Saved
            </button>
          </div>
          <span class="feed-view-count" data-testid="saved-count">
            {bookmarkedCount()} saved
          </span>
        </div>

        <Show when={detailSelection()}>
          {(value) => (
            <FeedDetailPanel
              selection={value()}
              rows={state.rows}
              canPublish={props.canPublish}
              followPubkeys={props.diagnostics?.followList ?? []}
              onClose={() => setSelection(null)}
            />
          )}
        </Show>

        {/* Timeline */}
        <div
          class="feed-timeline"
          data-testid={mode() === "saved" ? "saved-timeline" : "feed-timeline"}
        >
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
              when={visibleRows().length > 0}
              fallback={
                <div
                  class="feed-empty"
                  data-testid={mode() === "saved" ? "saved-empty" : "feed-empty"}
                >
                  <Show
                    when={mode() === "saved"}
                    fallback={
                      <EmptyFeedActions canPublish={props.canPublish} />
                    }
                  >
                    <EmptySavedActions syncing={bookmarkedCount() > 0} />
                  </Show>
                </div>
              }
            >
              <For each={visibleRows()}>
                {(row) => (
                  <PostCard
                    row={row}
                    canPublish={props.canPublish}
                    activeAccountPubkey={props.diagnostics?.activeAccountPubkey}
                    bookmarked={bookmarkedIds().has(row.id)}
                    onSelect={setSelection}
                    onQuote={startQuote}
                  />
                )}
              </For>
            </Show>
          </Show>
        </div>
      </div>
    </NostrProfileHostProvider>
  );
}

function EmptyFeedActions(props: { canPublish: boolean }) {
  return (
    <>
      <strong>No notes yet</strong>
      <span>Use discovery, relay checks, or identity setup to hydrate a real feed.</span>
      <div class="feed-empty-actions" aria-label="Feed next actions">
        <a href="#search">Search</a>
        <a href="#groups">Groups</a>
        <a href="#relays">Relays</a>
        <Show when={!props.canPublish}>
          <a href="#signing">Signer</a>
        </Show>
      </div>
    </>
  );
}

function EmptySavedActions(props: { syncing: boolean }) {
  return (
    <>
      <strong>{props.syncing ? "Saved notes are syncing" : "No saved notes yet"}</strong>
      <span>
        {props.syncing
          ? "Your bookmark list is loaded; waiting for those notes to hydrate from relays."
          : "Save a note from Home and it will appear here from the Rust bookmark projection."}
      </span>
      <div class="feed-empty-actions" aria-label="Saved next actions">
        <a href="#feed">Home</a>
        <a href="#search">Search</a>
      </div>
    </>
  );
}
