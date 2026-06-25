import { describe, expect, it } from "vitest";
import { chirpRelayOverrideFromSearch, chirpStartRelays } from "./chirpConfig";

describe("chirp relay overrides", () => {
  it("preserves role-explicit relay bootstrap entries from the query string", () => {
    const bootstrap = [
      ["ws://127.0.0.1:1001", "indexer"],
      ["ws://127.0.0.1:1002", "both,indexer"],
    ];

    const override = chirpRelayOverrideFromSearch(
      `?relay_bootstrap=${encodeURIComponent(JSON.stringify(bootstrap))}`,
    );
    const start = chirpStartRelays(override);

    expect(start.relays).toEqual(["ws://127.0.0.1:1001", "ws://127.0.0.1:1002"]);
    expect(start.relay_bootstrap).toEqual([
      { url: "ws://127.0.0.1:1001", role: "indexer" },
      { url: "ws://127.0.0.1:1002", role: "both,indexer" },
    ]);
  });

  it("keeps legacy relay URL overrides write-capable for one-relay smoke tests", () => {
    const override = chirpRelayOverrideFromSearch("?relay=ws%3A%2F%2F127.0.0.1%3A1003");
    const start = chirpStartRelays(override);

    expect(start.relays).toEqual(["ws://127.0.0.1:1003"]);
    expect(start.relay_bootstrap).toEqual([
      { url: "ws://127.0.0.1:1003", role: "both,indexer" },
    ]);
  });
});
