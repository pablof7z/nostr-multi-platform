import { afterEach, describe, expect, it, vi } from "vitest";
import type { WorkerRequest } from "@nmp/runtime-web";
import { fulfilSignRequestViaExtension } from "./signBroker";

const ACCOUNT = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const OTHER = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("fulfilSignRequestViaExtension", () => {
  it("fails closed before signEvent when the extension account differs", async () => {
    const signEvent = vi.fn();
    const post = vi.fn<(request: WorkerRequest) => void>();
    vi.stubGlobal("window", {
      nostr: {
        getPublicKey: vi.fn().mockResolvedValue(OTHER),
        signEvent,
      },
    });

    await fulfilSignRequestViaExtension(post, "sign-1", ACCOUNT, unsignedJson(ACCOUNT));

    expect(signEvent).not.toHaveBeenCalled();
    expect(post).toHaveBeenCalledWith({
      type: "deliver_signer_response",
      correlation_id: "sign-1",
      signed_json: null,
      error: expect.stringContaining("account mismatch"),
    });
  });

  it("calls signEvent only after the extension account matches the pinned account", async () => {
    const signed = { id: "11".repeat(32), pubkey: ACCOUNT, sig: "22".repeat(64) };
    const signEvent = vi.fn().mockResolvedValue(signed);
    const post = vi.fn<(request: WorkerRequest) => void>();
    vi.stubGlobal("window", {
      nostr: {
        getPublicKey: vi.fn().mockResolvedValue(ACCOUNT.toUpperCase()),
        signEvent,
      },
    });

    await fulfilSignRequestViaExtension(post, "sign-2", ACCOUNT, unsignedJson(ACCOUNT));

    expect(signEvent).toHaveBeenCalledWith(expect.objectContaining({ pubkey: ACCOUNT }));
    expect(post).toHaveBeenCalledWith({
      type: "deliver_signer_response",
      correlation_id: "sign-2",
      signed_json: JSON.stringify(signed),
      error: null,
    });
  });
});

function unsignedJson(pubkey: string): string {
  return JSON.stringify({
    pubkey,
    kind: 1,
    tags: [],
    content: "broker pinning",
    created_at: 1_700_000_000,
  });
}
