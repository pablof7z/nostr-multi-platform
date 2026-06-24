// ADR-0063 (#1671) — vitest for the resolve_ref/release_ref command builders.
//
// Pins the exact structured command shape the Rust resolver
// (`ref_dispatch_from_resolve` / `ref_dispatch_from_release`) expects to parse.
// Key/discriminant consistency between the TS builders and the Rust recognizer
// is the one cross-step failure mode that degrades silently to None (D6) and
// never fires — asserting the shape here catches it before it reaches Rust.
import { describe, expect, it } from "vitest";
import {
  REF_LIVENESS_CACHE_OK,
  REF_NS_EVENT,
  REF_NS_PROFILE,
  REF_SHAPE_EVENT_EMBED,
  REF_SHAPE_PROFILE_REF,
  releaseEventCommand,
  releaseProfileCommand,
  resolveEventCommand,
  resolveProfileCommand,
} from "./actions";

describe("ADR-0063 resolve_ref/release_ref command builders", () => {
  it("resolveProfileCommand produces the Lane D resolve_ref wire shape", () => {
    expect(resolveProfileCommand("abc123pubkey", "chirp-web-author-eventid1")).toEqual({
      kind: "resolve_ref",
      namespace: REF_NS_PROFILE,
      key: "abc123pubkey",
      consumerId: "chirp-web-author-eventid1",
      shape: REF_SHAPE_PROFILE_REF,
      liveness: REF_LIVENESS_CACHE_OK,
    });
  });

  it("releaseProfileCommand produces the release_ref wire shape (no shape/liveness)", () => {
    expect(releaseProfileCommand("abc123pubkey", "chirp-web-author-eventid1")).toEqual({
      kind: "release_ref",
      namespace: REF_NS_PROFILE,
      key: "abc123pubkey",
      consumerId: "chirp-web-author-eventid1",
    });
  });

  it("resolveEventCommand produces the event-namespace resolve_ref wire shape", () => {
    expect(resolveEventCommand("eventidhex", "chirp-web-embed-eventid2")).toEqual({
      kind: "resolve_ref",
      namespace: REF_NS_EVENT,
      key: "eventidhex",
      consumerId: "chirp-web-embed-eventid2",
      shape: REF_SHAPE_EVENT_EMBED,
      liveness: REF_LIVENESS_CACHE_OK,
    });
  });

  it("resolveEventCommand preserves optional relay hints", () => {
    expect(
      resolveEventCommand("eventidhex", "chirp-web-embed-eventid2", [
        "wss://relay.a.example",
        "wss://relay.b.example",
      ]),
    ).toEqual({
      kind: "resolve_ref",
      namespace: REF_NS_EVENT,
      key: "eventidhex",
      consumerId: "chirp-web-embed-eventid2",
      shape: REF_SHAPE_EVENT_EMBED,
      liveness: REF_LIVENESS_CACHE_OK,
      hints: ["wss://relay.a.example", "wss://relay.b.example"],
    });
  });

  it("releaseEventCommand produces the event-namespace release_ref wire shape", () => {
    expect(releaseEventCommand("eventidhex", "chirp-web-embed-eventid2")).toEqual({
      kind: "release_ref",
      namespace: REF_NS_EVENT,
      key: "eventidhex",
      consumerId: "chirp-web-embed-eventid2",
    });
  });

  it("consumerId is converted to consumer_id only at WorkerRequest construction", () => {
    const cmd = resolveProfileCommand("pk", "my-consumer");
    expect(cmd.kind).toBe("resolve_ref");
    if (cmd.kind !== "resolve_ref") return;
    expect(Object.keys(cmd)).toContain("consumerId");
    expect(Object.keys(cmd)).not.toContain("consumer_id");
    // `key` (not `pubkey`) is the unified namespace-agnostic field name.
    expect(cmd.key).toBe("pk");
  });

  it("profile and event resolves share the command kind but differ in namespace", () => {
    expect(resolveProfileCommand("pk", "c").kind).toBe("resolve_ref");
    expect(resolveEventCommand("ev", "c").kind).toBe("resolve_ref");
    const profile = resolveProfileCommand("pk", "c");
    const event = resolveEventCommand("ev", "c");
    if (profile.kind !== "resolve_ref" || event.kind !== "resolve_ref") return;
    expect(profile.namespace).toBe(REF_NS_PROFILE);
    expect(event.namespace).toBe(REF_NS_EVENT);
  });
});
