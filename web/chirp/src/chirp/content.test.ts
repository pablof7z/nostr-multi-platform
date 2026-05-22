import { describe, expect, it } from "vitest";
import { classifyEntity, shortEntity, tokenizeContent } from "./content";

describe("bech32 content fallback rendering", () => {
  it("classifies profile, note, event, address, and relay entities", () => {
    expect(classifyEntity("npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq")).toBe("profile");
    expect(classifyEntity("nprofile1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq")).toBe("profile");
    expect(classifyEntity("note1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq")).toBe("note");
    expect(classifyEntity("nevent1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq")).toBe("event");
    expect(classifyEntity("naddr1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq")).toBe("address");
    expect(classifyEntity("nrelay1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq")).toBe("relay");
  });

  it("keeps surrounding text while tokenizing embedded entities", () => {
    const tokens = tokenizeContent(
      "read note1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq by npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq",
    );

    expect(tokens).toEqual([
      { type: "text", value: "read " },
      {
        type: "entity",
        value: "note1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq",
        entityType: "note",
      },
      { type: "text", value: " by " },
      {
        type: "entity",
        value: "npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq",
        entityType: "profile",
      },
    ]);
  });

  it("marks nsec values as secrets and abbreviates long entity labels", () => {
    const nsec = "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";

    expect(tokenizeContent(nsec)).toEqual([{ type: "entity", value: nsec, entityType: "secret" }]);
    expect(shortEntity(nsec)).toBe("nsec1qqqqq...qqqqqq");
  });
});
