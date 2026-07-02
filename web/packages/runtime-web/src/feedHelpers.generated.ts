// -----------------------------------------------------------------------------
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen feed-helpers --platform ts \
//       --out web/packages/runtime-web/src/feedHelpers.generated.ts
//
// Source of truth: `crates/nmp-codegen/src/feed_helpers.rs`.
//
// These helpers build canonical FeedParams JSON and call the runtime-web
// feed_open_json Worker control. They do not own feed reactivity, compiler
// selection, or session teardown; Rust/NMP does.
// -----------------------------------------------------------------------------

import type { FeedSessionHandle, WorkerEvent, WorkerRequest } from "./protocol";

export type FeedHelperShape = "RootIndexed" | "Flat";

export type FeedRuntime = {
  handle(request: WorkerRequest): WorkerEvent[];
  openFeedJson?: (paramsJson: string, correlationId: string) => FeedSessionHandle | undefined;
};

export const GeneratedFeedHelpers = {
  activeUserFollowsFeedParamsJson(
    feedKey: string,
    primaryKinds: number[],
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): string {
    return JSON.stringify({
      primary_kinds: primaryKinds,
      shape,
      source: "ActiveUserFollows",
      admission: "All",
      order: "NewestByFeedPosition",
      window: {
        initial_limit: visibleLimit,
        page_size: visibleLimit,
        source_page_size: visibleLimit,
      },
      key: feedKey,
      item_projection: "FeedRows",
    });
  },

  openActiveUserFollowsFeedRequest(
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): WorkerRequest {
    return {
      type: "feed_open_json",
      params_json: GeneratedFeedHelpers.activeUserFollowsFeedParamsJson(
        feedKey,
        primaryKinds,
        visibleLimit,
        shape,
      ),
      correlation_id: correlationId,
    };
  },

  openActiveUserFollowsFeed(
    runtime: FeedRuntime,
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): FeedSessionHandle | undefined {
    const paramsJson = GeneratedFeedHelpers.activeUserFollowsFeedParamsJson(
      feedKey,
      primaryKinds,
      visibleLimit,
      shape,
    );
    if (typeof runtime.openFeedJson === "function") {
      return runtime.openFeedJson(paramsJson, correlationId);
    }
    return feedHandleFromEvents(
      runtime.handle({
        type: "feed_open_json",
        params_json: paramsJson,
        correlation_id: correlationId,
      }),
    );
  },

  loadOlderRequest(handle: FeedSessionHandle, correlationId: string): WorkerRequest {
    return { type: "feed_load_older", handle, correlation_id: correlationId };
  },

  closeRequest(handle: FeedSessionHandle, correlationId: string): WorkerRequest {
    return { type: "feed_close", handle, correlation_id: correlationId };
  },
};

function feedHandleFromEvents(events: WorkerEvent[]): FeedSessionHandle | undefined {
  return events.find((event) => event.type === "feed_opened")?.handle;
}
