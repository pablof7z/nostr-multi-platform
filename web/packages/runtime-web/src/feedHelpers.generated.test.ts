import { describe, expect, it } from "vitest";

import { GeneratedFeedHelpers } from "./feedHelpers.generated";
import type { WorkerEvent, WorkerRequest } from "./protocol";

describe("GeneratedFeedHelpers", () => {
  it("builds the canonical active-user-follows feed request", () => {
    const request = GeneratedFeedHelpers.openActiveUserFollowsFeedRequest(
      "feed-1",
      "app.web.home",
      [1, 20],
      40,
      "Flat",
    );

    expect(request.type).toBe("feed_open_json");
    if (request.type !== "feed_open_json") throw new Error("expected feed_open_json request");
    expect(request.correlation_id).toBe("feed-1");

    const params = JSON.parse(request.params_json) as Record<string, unknown>;
    expect(params).toMatchObject({
      primary_kinds: [1, 20],
      shape: "Flat",
      source: "ActiveUserFollows",
      admission: "All",
      order: "NewestByFeedPosition",
      key: "app.web.home",
      item_projection: "FeedRows",
    });
    expect(params.window).toMatchObject({
      initial_limit: 40,
      page_size: 40,
      source_page_size: 40,
    });
  });

  it("extracts the opened feed handle from runtime events", () => {
    const calls: WorkerRequest[] = [];
    const runtime = {
      handle(request: WorkerRequest): WorkerEvent[] {
        calls.push(request);
        return [
          {
            type: "feed_opened",
            correlation_id: "feed-2",
            handle: { projection_key: "app.web.home", session_id: 7 },
          },
        ];
      },
    };

    const handle = GeneratedFeedHelpers.openActiveUserFollowsFeed(
      runtime,
      "feed-2",
      "app.web.home",
      [1],
    );

    expect(calls[0]?.type).toBe("feed_open_json");
    expect(handle).toEqual({ projection_key: "app.web.home", session_id: 7 });
  });
});
