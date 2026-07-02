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
            handle: { projection_key: "app.web.home", handle_id: 7 },
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
    expect(handle).toEqual({ projection_key: "app.web.home", handle_id: 7 });
  });

  it("builds the canonical hosted-groups feed request", () => {
    const request = GeneratedFeedHelpers.openHostedGroupsFeedRequest(
      "feed-3",
      "app.web.groups",
      [9],
    );
    if (request.type !== "feed_open_json") throw new Error("expected feed_open_json request");
    const params = JSON.parse(request.params_json) as Record<string, unknown>;
    expect(params).toMatchObject({
      primary_kinds: [9],
      source: "ActiveUserHostedGroups",
      key: "app.web.groups",
    });
  });

  it("builds the canonical list-members feed request", () => {
    const request = GeneratedFeedHelpers.openListMembersFeedRequest(
      "feed-4",
      "app.web.list",
      [1],
      "fiatjaf:10000",
    );
    if (request.type !== "feed_open_json") throw new Error("expected feed_open_json request");
    const params = JSON.parse(request.params_json) as Record<string, unknown>;
    expect(params).toMatchObject({
      primary_kinds: [1],
      source: { ListMembers: { list: "fiatjaf:10000" } },
      key: "app.web.list",
    });
  });

  it("builds the canonical relay-set feed request", () => {
    const request = GeneratedFeedHelpers.openRelaySetFeedRequest(
      "feed-5",
      "app.web.relayset",
      [30023],
      "network-relays",
    );
    if (request.type !== "feed_open_json") throw new Error("expected feed_open_json request");
    const params = JSON.parse(request.params_json) as Record<string, unknown>;
    expect(params).toMatchObject({
      primary_kinds: [30023],
      source: { RelaySet: { relays: "network-relays" } },
      key: "app.web.relayset",
    });
  });
});
