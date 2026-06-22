// ADR-0063 (#1671) — vitest for the resolve_ref/release_ref command builders.
//
// Pins the exact wire shape the Rust dispatch recognizer
// (`resolve_dispatch_from_action` in crates/nmp-wasm/src/dispatch_routing.rs)
// expects to parse. Key/discriminant consistency between the TS builders and the
// Rust recognizer is the one cross-step failure mode that degrades silently to
// None (D6) and never fires — asserting the shape here catches it before it
// reaches the Rust layer.
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
      actionType: "nmp.kernel.resolve_ref",
      payload: {
        namespace: REF_NS_PROFILE,
        key: "abc123pubkey",
        consumer_id: "chirp-web-author-eventid1",
        shape: REF_SHAPE_PROFILE_REF,
        liveness: REF_LIVENESS_CACHE_OK,
      },
    });
  });

  it("releaseProfileCommand produces the release_ref wire shape (no shape/liveness)", () => {
    expect(releaseProfileCommand("abc123pubkey", "chirp-web-author-eventid1")).toEqual({
      actionType: "nmp.kernel.release_ref",
      payload: {
        namespace: REF_NS_PROFILE,
        key: "abc123pubkey",
        consumer_id: "chirp-web-author-eventid1",
      },
    });
  });

  it("resolveEventCommand produces the event-namespace resolve_ref wire shape", () => {
    expect(resolveEventCommand("eventidhex", "chirp-web-embed-eventid2")).toEqual({
      actionType: "nmp.kernel.resolve_ref",
      payload: {
        namespace: REF_NS_EVENT,
        key: "eventidhex",
        consumer_id: "chirp-web-embed-eventid2",
        shape: REF_SHAPE_EVENT_EMBED,
        liveness: REF_LIVENESS_CACHE_OK,
      },
    });
  });

  it("releaseEventCommand produces the event-namespace release_ref wire shape", () => {
    expect(releaseEventCommand("eventidhex", "chirp-web-embed-eventid2")).toEqual({
      actionType: "nmp.kernel.release_ref",
      payload: {
        namespace: REF_NS_EVENT,
        key: "eventidhex",
        consumer_id: "chirp-web-embed-eventid2",
      },
    });
  });

  it("consumer_id key is snake_case (Rust parser expects consumer_id not consumerId)", () => {
    // The Rust `str_field(&payload, "consumer_id")` call requires exactly this
    // key. A camelCase mismatch would produce None from the recognizer and the
    // resolve would silently fall through to write-path-unavailable.
    const cmd = resolveProfileCommand("pk", "my-consumer");
    const payload = cmd.payload as Record<string, unknown>;
    expect(Object.keys(payload)).toContain("consumer_id");
    expect(Object.keys(payload)).not.toContain("consumerId");
    // `key` (not `pubkey`) is the unified namespace-agnostic field name.
    expect(payload.key).toBe("pk");
  });

  it("profile and event resolves share the action_type but differ in namespace", () => {
    expect(resolveProfileCommand("pk", "c").actionType).toBe("nmp.kernel.resolve_ref");
    expect(resolveEventCommand("ev", "c").actionType).toBe("nmp.kernel.resolve_ref");
    const profile = resolveProfileCommand("pk", "c").payload as Record<string, unknown>;
    const event = resolveEventCommand("ev", "c").payload as Record<string, unknown>;
    expect(profile.namespace).toBe(REF_NS_PROFILE);
    expect(event.namespace).toBe(REF_NS_EVENT);
  });
});
