import { describe, expect, it } from "vitest";
import type { WorkerEvent } from "@nmp/runtime-web";
import { deriveSignLifecycle, latestCapabilityFailure } from "./signerStatus";

// Events are newest-first (the shell prepends), so these arrays are written
// newest → oldest.

describe("deriveSignLifecycle", () => {
  it("returns idle on an empty log", () => {
    expect(deriveSignLifecycle([])).toEqual({ phase: "idle" });
  });

  it("returns pending when the latest sign event is a request", () => {
    const events: WorkerEvent[] = [
      { type: "sign_request", correlation_id: "c1", account_pubkey: "ab", unsigned_json: "{}" },
      { type: "update_bytes", bytes: new Uint8Array() },
    ];
    expect(deriveSignLifecycle(events)).toEqual({
      phase: "pending",
      correlationId: "c1",
      accountPubkey: "ab",
    });
  });

  it("returns completed when the latest sign event is a completion", () => {
    const events: WorkerEvent[] = [
      { type: "sign_completed", correlation_id: "c1", signed_json: "{}" },
      { type: "sign_request", correlation_id: "c1", account_pubkey: "ab", unsigned_json: "{}" },
    ];
    expect(deriveSignLifecycle(events)).toEqual({ phase: "completed", correlationId: "c1" });
  });

  it("returns failed with the reason when the latest sign event is a failure", () => {
    const events: WorkerEvent[] = [
      { type: "sign_failed", correlation_id: "c1", reason: "user rejected" },
    ];
    expect(deriveSignLifecycle(events)).toEqual({
      phase: "failed",
      correlationId: "c1",
      reason: "user rejected",
    });
  });
});

describe("latestCapabilityFailure", () => {
  it("surfaces the most recent capability failure", () => {
    const events: WorkerEvent[] = [
      { type: "capability_failure", capability: "nmp.set_identity", correlation_id: "c1", reason: "boom" },
    ];
    expect(latestCapabilityFailure(events)).toBe("boom");
  });

  it("returns undefined when a positive signal is newer than the failure", () => {
    const events: WorkerEvent[] = [
      { type: "action_accepted", action_type: "nmp.set_identity", correlation_id: "c2" },
      { type: "capability_failure", capability: "nmp.set_identity", correlation_id: "c1", reason: "boom" },
    ];
    expect(latestCapabilityFailure(events)).toBeUndefined();
  });

  it("returns undefined on an empty log", () => {
    expect(latestCapabilityFailure([])).toBeUndefined();
  });
});
