import { describe, expect, it } from "vitest";
import { checkNsecFormat } from "./nsecInput";

describe("checkNsecFormat", () => {
  // A real-shaped (but non-secret, all-data-char) nsec1 string of valid length.
  const wellFormed = `nsec1${"q".repeat(58)}`;

  it("rejects an empty string", () => {
    expect(checkNsecFormat("   ")).toEqual({ ok: false, reason: expect.any(String) });
  });

  it("rejects a non-nsec prefix (e.g. npub)", () => {
    const res = checkNsecFormat(`npub1${"q".repeat(58)}`);
    expect(res.ok).toBe(false);
  });

  it("rejects an implausibly short key", () => {
    expect(checkNsecFormat("nsec1qqqq").ok).toBe(false);
  });

  it("rejects non-bech32 characters in the data part", () => {
    // 'b', 'i', 'o', '1' are NOT in the bech32 charset.
    const res = checkNsecFormat(`nsec1${"b".repeat(58)}`);
    expect(res.ok).toBe(false);
  });

  it("accepts a well-formed lowercase nsec and trims whitespace", () => {
    const res = checkNsecFormat(`  ${wellFormed}  `);
    expect(res).toEqual({ ok: true, value: wellFormed });
  });
});
