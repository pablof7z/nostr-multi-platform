import { describe, expect, it } from "vitest";
import {
  DISPATCH_ENVELOPE_FILE_IDENTIFIER,
  decodeDispatchEnvelopeRouting,
  encodeDispatchEnvelope,
} from "./dispatchEnvelope";

describe("dispatch envelope (ADR-0071 typed write transport)", () => {
  it("stamps the NMPD file identifier at byte offset 4", () => {
    // Cross-host wire contract: the Rust decoder
    // (nmp_core::dispatch_envelope::decode_dispatch_envelope) reads the raw
    // 4 bytes at offset 4 BEFORE any FlatBuffers traversal. If this drifts the
    // wasm runtime fails closed with `dispatch_envelope_rejected`.
    const bytes = encodeDispatchEnvelope("corr-1", "nmp.publish", new Uint8Array([1, 2, 3]));
    const id = String.fromCharCode(bytes[4], bytes[5], bytes[6], bytes[7]);
    expect(id).toBe(DISPATCH_ENVELOPE_FILE_IDENTIFIER);
    expect(id).toBe("NMPD");
  });

  it("round-trips the routing fields (correlation_id + action_namespace)", () => {
    const bytes = encodeDispatchEnvelope("corr-42", "nmp.nip25.react", new Uint8Array([9, 9]));
    const routing = decodeDispatchEnvelopeRouting(bytes);
    expect(routing).not.toBeNull();
    expect(routing?.correlationId).toBe("corr-42");
    expect(routing?.actionNamespace).toBe("nmp.nip25.react");
  });

  it("rejects a buffer that is not a DispatchEnvelope root", () => {
    expect(decodeDispatchEnvelopeRouting(new TextEncoder().encode("not a flatbuffer"))).toBeNull();
    expect(decodeDispatchEnvelopeRouting(new Uint8Array([0, 1, 2]))).toBeNull();
  });
});
