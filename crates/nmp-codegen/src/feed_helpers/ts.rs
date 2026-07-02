//! TypeScript feed-helper emitter.

/// Render `feedHelpers.generated.ts`.
#[must_use]
pub fn render() -> String {
    r#"// -----------------------------------------------------------------------------
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

function buildFeedParamsJson(
  feedKey: string,
  primaryKinds: number[],
  source: unknown,
  visibleLimit: number,
  shape: FeedHelperShape,
): string {
  return JSON.stringify({
    primary_kinds: primaryKinds,
    shape,
    source,
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
}

function feedHandleFromEvents(events: WorkerEvent[]): FeedSessionHandle | undefined {
  return events.find((event) => event.type === "feed_opened")?.handle;
}

function openFeedRequest(paramsJson: string, correlationId: string): WorkerRequest {
  return { type: "feed_open_json", params_json: paramsJson, correlation_id: correlationId };
}

function openFeed(
  runtime: FeedRuntime,
  correlationId: string,
  paramsJson: string,
): FeedSessionHandle | undefined {
  if (typeof runtime.openFeedJson === "function") {
    return runtime.openFeedJson(paramsJson, correlationId);
  }
  return feedHandleFromEvents(runtime.handle(openFeedRequest(paramsJson, correlationId)));
}

export const GeneratedFeedHelpers = {
  activeUserFollowsFeedParamsJson(
    feedKey: string,
    primaryKinds: number[],
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): string {
    return buildFeedParamsJson(feedKey, primaryKinds, "ActiveUserFollows", visibleLimit, shape);
  },

  openActiveUserFollowsFeedRequest(
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): WorkerRequest {
    return openFeedRequest(
      GeneratedFeedHelpers.activeUserFollowsFeedParamsJson(feedKey, primaryKinds, visibleLimit, shape),
      correlationId,
    );
  },

  openActiveUserFollowsFeed(
    runtime: FeedRuntime,
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): FeedSessionHandle | undefined {
    return openFeed(
      runtime,
      correlationId,
      GeneratedFeedHelpers.activeUserFollowsFeedParamsJson(feedKey, primaryKinds, visibleLimit, shape),
    );
  },

  /** The active account's hosted-group set. See `FeedSourceExpr::ActiveUserHostedGroups`. */
  hostedGroupsFeedParamsJson(
    feedKey: string,
    primaryKinds: number[],
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): string {
    return buildFeedParamsJson(feedKey, primaryKinds, "ActiveUserHostedGroups", visibleLimit, shape);
  },

  openHostedGroupsFeedRequest(
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): WorkerRequest {
    return openFeedRequest(
      GeneratedFeedHelpers.hostedGroupsFeedParamsJson(feedKey, primaryKinds, visibleLimit, shape),
      correlationId,
    );
  },

  openHostedGroupsFeed(
    runtime: FeedRuntime,
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): FeedSessionHandle | undefined {
    return openFeed(
      runtime,
      correlationId,
      GeneratedFeedHelpers.hostedGroupsFeedParamsJson(feedKey, primaryKinds, visibleLimit, shape),
    );
  },

  /** Members of an app/defaults-registered list id. See `FeedSourceExpr::ListMembers`. */
  listMembersFeedParamsJson(
    feedKey: string,
    primaryKinds: number[],
    listId: string,
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): string {
    return buildFeedParamsJson(feedKey, primaryKinds, { ListMembers: { list: listId } }, visibleLimit, shape);
  },

  openListMembersFeedRequest(
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    listId: string,
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): WorkerRequest {
    return openFeedRequest(
      GeneratedFeedHelpers.listMembersFeedParamsJson(feedKey, primaryKinds, listId, visibleLimit, shape),
      correlationId,
    );
  },

  openListMembersFeed(
    runtime: FeedRuntime,
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    listId: string,
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): FeedSessionHandle | undefined {
    return openFeed(
      runtime,
      correlationId,
      GeneratedFeedHelpers.listMembersFeedParamsJson(feedKey, primaryKinds, listId, visibleLimit, shape),
    );
  },

  /** An app-registered relay set. See `FeedSourceExpr::RelaySet`. */
  relaySetFeedParamsJson(
    feedKey: string,
    primaryKinds: number[],
    relaySetId: string,
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): string {
    return buildFeedParamsJson(feedKey, primaryKinds, { RelaySet: { relays: relaySetId } }, visibleLimit, shape);
  },

  openRelaySetFeedRequest(
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    relaySetId: string,
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): WorkerRequest {
    return openFeedRequest(
      GeneratedFeedHelpers.relaySetFeedParamsJson(feedKey, primaryKinds, relaySetId, visibleLimit, shape),
      correlationId,
    );
  },

  openRelaySetFeed(
    runtime: FeedRuntime,
    correlationId: string,
    feedKey: string,
    primaryKinds: number[],
    relaySetId: string,
    visibleLimit = 80,
    shape: FeedHelperShape = "RootIndexed",
  ): FeedSessionHandle | undefined {
    return openFeed(
      runtime,
      correlationId,
      GeneratedFeedHelpers.relaySetFeedParamsJson(feedKey, primaryKinds, relaySetId, visibleLimit, shape),
    );
  },

  loadOlderRequest(handle: FeedSessionHandle, correlationId: string): WorkerRequest {
    return { type: "feed_load_older", handle, correlation_id: correlationId };
  },

  closeRequest(handle: FeedSessionHandle, correlationId: string): WorkerRequest {
    return { type: "feed_close", handle, correlation_id: correlationId };
  },
};
"#
    .to_string()
}
