import { describe, expect, it } from "vitest";

import {
  asSignRequest,
  fulfilSignRequest,
  installNip07SignBroker,
  type Nip07Signer,
  type SignRequest,
} from "./signBroker";
import type { WorkerEvent, WorkerRequest } from "./protocol";

const REQUEST: SignRequest = {
  correlationId: "corr-1",
  accountPubkey: "pubkey-a",
  unsignedJson: JSON.stringify({ kind: 1, content: "hi" }),
};

describe("asSignRequest", () => {
  it("extracts the sign_request fields", () => {
    const event: WorkerEvent = {
      type: "sign_request",
      correlation_id: "corr-1",
      account_pubkey: "pubkey-a",
      unsigned_json: "{}",
    };
    expect(asSignRequest(event)).toEqual({
      correlationId: "corr-1",
      accountPubkey: "pubkey-a",
      unsignedJson: "{}",
    });
  });

  it("returns undefined for a different event type", () => {
    const event: WorkerEvent = { type: "hello_accepted", protocol_version: 1, status: "ready" };
    expect(asSignRequest(event)).toBeUndefined();
  });
});

describe("fulfilSignRequest", () => {
  it("fails closed when the signer's active account does not match the request", async () => {
    const signer: Nip07Signer = {
      publicKey: async () => "pubkey-DIFFERENT",
      sign: async (e) => e,
    };
    const posted: WorkerRequest[] = [];
    await fulfilSignRequest((r) => posted.push(r), signer, REQUEST);
    const req = posted[0];
    if (req.type !== "deliver_signer_response") throw new Error("expected deliver_signer_response");
    expect(req.correlation_id).toBe("corr-1");
    expect(req.signed_json).toBeNull();
    expect(req.error).toMatch(/different account/);
  });

  it("fails closed when the unsigned JSON does not parse", async () => {
    const signer: Nip07Signer = {
      publicKey: async () => "pubkey-a",
      sign: async (e) => e,
    };
    const posted: WorkerRequest[] = [];
    await fulfilSignRequest((r) => posted.push(r), signer, { ...REQUEST, unsignedJson: "not json" });
    const req = posted[0];
    if (req.type !== "deliver_signer_response") throw new Error("expected deliver_signer_response");
    expect(req.signed_json).toBeNull();
    expect(req.error).toMatch(/did not parse/);
  });

  it("delivers the signed event on success (account match is case-insensitive)", async () => {
    const signed = { id: "abc", sig: "def" };
    const signer: Nip07Signer = {
      publicKey: async () => "PubKey-A",
      sign: async (event) => ({ ...event, ...signed }),
    };
    const posted: WorkerRequest[] = [];
    await fulfilSignRequest((r) => posted.push(r), signer, REQUEST);
    const req = posted[0];
    if (req.type !== "deliver_signer_response") throw new Error("expected deliver_signer_response");
    expect(req.error).toBeNull();
    expect(JSON.parse(req.signed_json ?? "{}")).toMatchObject(signed);
  });

  it("fails closed when the signer rejects publicKey()", async () => {
    const signer: Nip07Signer = {
      publicKey: async () => {
        throw new Error("no extension installed");
      },
      sign: async (e) => e,
    };
    const posted: WorkerRequest[] = [];
    await fulfilSignRequest((r) => posted.push(r), signer, REQUEST);
    const req = posted[0];
    if (req.type !== "deliver_signer_response") throw new Error("expected deliver_signer_response");
    expect(req.signed_json).toBeNull();
    expect(req.error).toMatch(/publicKey\(\) rejected/);
  });

  it("fails closed when sign() rejects (user cancels)", async () => {
    const signer: Nip07Signer = {
      publicKey: async () => "pubkey-a",
      sign: async () => {
        throw new Error("user rejected");
      },
    };
    const posted: WorkerRequest[] = [];
    await fulfilSignRequest((r) => posted.push(r), signer, REQUEST);
    const req = posted[0];
    if (req.type !== "deliver_signer_response") throw new Error("expected deliver_signer_response");
    expect(req.signed_json).toBeNull();
    expect(req.error).toMatch(/rejected/);
  });
});

describe("installNip07SignBroker", () => {
  it("fulfils a sign_request message and returns an unsubscribe function", async () => {
    const signer: Nip07Signer = {
      publicKey: async () => "pubkey-a",
      sign: async (event) => ({ ...event, sig: "sig" }),
    };
    const listeners: ((event: MessageEvent) => void)[] = [];
    const posted: WorkerRequest[] = [];
    const worker = {
      addEventListener: (_type: "message", listener: (event: MessageEvent) => void) => {
        listeners.push(listener);
      },
      removeEventListener: (_type: "message", listener: (event: MessageEvent) => void) => {
        const i = listeners.indexOf(listener);
        if (i >= 0) listeners.splice(i, 1);
      },
      postMessage: (r: WorkerRequest) => posted.push(r),
    };

    const unsubscribe = installNip07SignBroker(worker, signer);
    expect(listeners).toHaveLength(1);

    listeners[0]({
      data: {
        type: "sign_request",
        correlation_id: "corr-6",
        account_pubkey: "pubkey-a",
        unsigned_json: "{}",
      },
    } as MessageEvent);
    // fulfilSignRequest is async; flush microtasks.
    await Promise.resolve();
    await Promise.resolve();

    expect(posted).toHaveLength(1);
    expect(posted[0].type).toBe("deliver_signer_response");

    unsubscribe();
    expect(listeners).toHaveLength(0);
  });

  it("ignores non-sign_request messages", () => {
    const signer: Nip07Signer = { publicKey: async () => "pubkey-a", sign: async (e) => e };
    const listeners: ((event: MessageEvent) => void)[] = [];
    const posted: WorkerRequest[] = [];
    const worker = {
      addEventListener: (_type: "message", listener: (event: MessageEvent) => void) => {
        listeners.push(listener);
      },
      removeEventListener: () => {},
      postMessage: (r: WorkerRequest) => posted.push(r),
    };
    installNip07SignBroker(worker, signer);
    listeners[0]({ data: { type: "hello_accepted" } } as MessageEvent);
    expect(posted).toHaveLength(0);
  });
});
