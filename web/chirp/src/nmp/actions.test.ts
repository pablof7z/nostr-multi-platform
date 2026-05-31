// F-CR-00 — vitest for claim/release command builders.
//
// Pins the exact wire shape the Rust dispatch recognizer
// (`claim_dispatch_from_action`) expects to parse. Key consistency between
// Step 4 (builders) and Step 2 (recognizer) is the one cross-step failure mode
// that degrades silently to None (D6) and never fires — asserting shape here
// catches it before it reaches the Rust layer.
import { describe, expect, it } from "vitest";
import {
  claimEventCommand,
  claimProfileCommand,
  releaseEventCommand,
  releaseProfileCommand,
} from "./actions";

describe("F-CR-00 claim/release command builders", () => {
  it("claimProfileCommand produces expected wire shape", () => {
    expect(claimProfileCommand("abc123pubkey", "chirp-web-author-eventid1")).toEqual({
      actionType: "nmp.kernel.claim_profile",
      payload: {
        pubkey: "abc123pubkey",
        consumer_id: "chirp-web-author-eventid1",
      },
    });
  });

  it("releaseProfileCommand produces expected wire shape", () => {
    expect(releaseProfileCommand("abc123pubkey", "chirp-web-author-eventid1")).toEqual({
      actionType: "nmp.kernel.release_profile",
      payload: {
        pubkey: "abc123pubkey",
        consumer_id: "chirp-web-author-eventid1",
      },
    });
  });

  it("claimEventCommand produces expected wire shape", () => {
    expect(claimEventCommand("nostr:nevent1xyz", "chirp-web-embed-eventid2")).toEqual({
      actionType: "nmp.kernel.claim_event",
      payload: {
        uri: "nostr:nevent1xyz",
        consumer_id: "chirp-web-embed-eventid2",
      },
    });
  });

  it("releaseEventCommand produces expected wire shape", () => {
    expect(releaseEventCommand("nostr:nevent1xyz", "chirp-web-embed-eventid2")).toEqual({
      actionType: "nmp.kernel.release_event",
      payload: {
        uri: "nostr:nevent1xyz",
        consumer_id: "chirp-web-embed-eventid2",
      },
    });
  });

  it("consumer_id key is snake_case (Rust parser expects consumer_id not consumerId)", () => {
    // The Rust `str_field(&payload, "consumer_id")` call requires exactly this
    // key. A camelCase mismatch would produce None from the recognizer and the
    // claim would silently fall through to write-path-unavailable.
    const cmd = claimProfileCommand("pk", "my-consumer");
    const payload = cmd.payload as Record<string, unknown>;
    expect(Object.keys(payload)).toContain("consumer_id");
    expect(Object.keys(payload)).not.toContain("consumerId");
  });

  it("claim_profile and claim_event use distinct action_type prefixes", () => {
    expect(claimProfileCommand("pk", "c").actionType).toBe("nmp.kernel.claim_profile");
    expect(claimEventCommand("nostr:note1abc", "c").actionType).toBe("nmp.kernel.claim_event");
  });
});
