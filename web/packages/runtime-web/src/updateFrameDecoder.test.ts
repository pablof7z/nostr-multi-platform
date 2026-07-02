import { describe, expect, it } from "vitest";
import * as flatbuffers from "flatbuffers";

import { FrameKind } from "./generated/nmp/transport/frame-kind";
import { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";
import { TypedPayload } from "./generated/nmp/transport/typed-payload";
import { TypedProjection } from "./generated/nmp/transport/typed-projection";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";
import { PROJECTION_CONTRACT } from "./projectionContract.generated";
import { decodeUpdateFrame } from "./updateFrameDecoder";

type ProjectionSpec = { key: string; schemaId: string; fileIdentifier: string; payload?: Uint8Array };

function encodeUpdateFrame(
  sessionId: bigint,
  snapshotEpoch: bigint,
  running: boolean,
  projections: ProjectionSpec[],
): Uint8Array {
  const builder = new flatbuffers.Builder();
  const projOffsets = projections.map((p) => {
    const schemaIdOff = builder.createString(p.schemaId);
    const fileIdOff = builder.createString(p.fileIdentifier);
    const payloadOff = p.payload
      ? TypedPayload.createPayloadVector(builder, p.payload)
      : TypedPayload.createPayloadVector(builder, new Uint8Array());
    TypedPayload.startTypedPayload(builder);
    TypedPayload.addSchemaId(builder, schemaIdOff);
    TypedPayload.addFileIdentifier(builder, fileIdOff);
    TypedPayload.addPayload(builder, payloadOff);
    const payloadTable = TypedPayload.endTypedPayload(builder);

    const keyOff = builder.createString(p.key);
    TypedProjection.startTypedProjection(builder);
    TypedProjection.addKey(builder, keyOff);
    TypedProjection.addPayload(builder, payloadTable);
    return TypedProjection.endTypedProjection(builder);
  });
  const projVec = SnapshotFrame.createTypedProjectionsVector(builder, projOffsets);

  SnapshotFrame.startSnapshotFrame(builder);
  SnapshotFrame.addSessionId(builder, sessionId);
  SnapshotFrame.addSnapshotEpoch(builder, snapshotEpoch);
  SnapshotFrame.addRunning(builder, running);
  SnapshotFrame.addTypedProjections(builder, projVec);
  const snapshotOff = SnapshotFrame.endSnapshotFrame(builder);

  UpdateFrame.startUpdateFrame(builder);
  UpdateFrame.addKind(builder, FrameKind.Snapshot);
  UpdateFrame.addSnapshot(builder, snapshotOff);
  const root = UpdateFrame.endUpdateFrame(builder);
  UpdateFrame.finishUpdateFrameBuffer(builder, root);
  return builder.asUint8Array().slice();
}

describe("decodeUpdateFrame (#2722)", () => {
  it("decodes the projection-key set + identity fields from a Snapshot frame", () => {
    const contract = PROJECTION_CONTRACT["refs.profile"];
    const bytes = encodeUpdateFrame(7n, 3n, true, [
      { key: "refs.profile", schemaId: contract.schemaId, fileIdentifier: contract.fileIdentifier, payload: new Uint8Array([1, 2, 3]) },
    ]);
    const frame = decodeUpdateFrame(bytes);
    expect(frame).toBeDefined();
    expect(frame?.projectionKeys).toEqual(["refs.profile"]);
    expect(frame?.sessionId).toBe(7n);
    expect(frame?.snapshotEpoch).toBe(3n);
    expect(frame?.running).toBe(true);
  });

  it("findTypedProjection returns the payload bytes when the identity matches the contract", () => {
    const contract = PROJECTION_CONTRACT["refs.profile"];
    const bytes = encodeUpdateFrame(1n, 1n, false, [
      {
        key: "refs.profile",
        schemaId: contract.schemaId,
        fileIdentifier: contract.fileIdentifier,
        payload: new Uint8Array([9, 8, 7]),
      },
    ]);
    const frame = decodeUpdateFrame(bytes);
    expect(frame?.findTypedProjection("refs.profile")).toEqual(new Uint8Array([9, 8, 7]));
  });

  it("findTypedProjection fails closed on an undeclared key", () => {
    const bytes = encodeUpdateFrame(1n, 1n, false, []);
    const frame = decodeUpdateFrame(bytes);
    expect(frame?.findTypedProjection("nmp.not.a.real.key")).toBeUndefined();
  });

  it("findTypedProjection fails closed when the sidecar's fileIdentifier does not match the contract", () => {
    const bytes = encodeUpdateFrame(1n, 1n, false, [
      { key: "refs.profile", schemaId: "refs.profile", fileIdentifier: "WRNG", payload: new Uint8Array([1]) },
    ]);
    const frame = decodeUpdateFrame(bytes);
    expect(frame?.findTypedProjection("refs.profile")).toBeUndefined();
  });

  it("findTypedProjection fails closed on an empty payload", () => {
    const contract = PROJECTION_CONTRACT["refs.profile"];
    const bytes = encodeUpdateFrame(1n, 1n, false, [
      { key: "refs.profile", schemaId: contract.schemaId, fileIdentifier: contract.fileIdentifier },
    ]);
    const frame = decodeUpdateFrame(bytes);
    expect(frame?.findTypedProjection("refs.profile")).toBeUndefined();
  });

  it("returns undefined for a garbage (non-NMPU) buffer", () => {
    expect(decodeUpdateFrame(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]))).toBeUndefined();
  });
});
