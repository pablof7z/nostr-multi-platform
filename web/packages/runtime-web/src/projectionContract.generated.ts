// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen projection-contract --platform ts \
//       --out web/packages/runtime-web/src/projectionContract.generated.ts
//
// Source of truth: PROJECTION_CONTRACT in
// `crates/nmp-codegen/src/projection_contract/table.rs`.
// The CI gate (`.github/workflows/codegen-drift.yml`) fails any PR whose
// generated TypeScript differs.
//
// #2722 — the neutral wire identity (schema_id + file_identifier) of every
// projection the kernel/host can emit, keyed by the `TypedProjection.key` a
// `SnapshotFrame.typed_projections` entry carries. Consumed by
// `updateFrameDecoder.ts`'s `findTypedProjection` to verify a sidecar's
// identity before handing its bytes to a decoder — never hand-copied per call
// site (the historical hl/nmp-gallery pattern this replaces).
// ─────────────────────────────────────────────────────────────────────────────

/** One projection's neutral wire identity. */
export type ProjectionContractEntry = {
  /** `TypedPayload.schema_id` carried on the sidecar buffer. */
  schemaId: string;
  /** FlatBuffers `file_identifier` for the sidecar's root table. */
  fileIdentifier: string;
};

/** `TypedProjection.key` -> neutral wire identity, for every projection the
 *  system emits (kernel built-ins + host-registered + keyed row-delta
 *  carriers). */
export const PROJECTION_CONTRACT: Readonly<Record<string, ProjectionContractEntry>> = {
  "profile": { schemaId: "profile", fileIdentifier: "KPRF" },
  "accounts": { schemaId: "accounts", fileIdentifier: "KACC" },
  "active_account": { schemaId: "active_account", fileIdentifier: "KACT" },
  "configured_relays": { schemaId: "configured_relays", fileIdentifier: "KCRL" },
  "relay_role_options": { schemaId: "relay_role_options", fileIdentifier: "KRRO" },
  "settings_hub": { schemaId: "settings_hub", fileIdentifier: "KSHB" },
  "publish_queue": { schemaId: "publish_queue", fileIdentifier: "KPBQ" },
  "publish_outbox": { schemaId: "publish_outbox", fileIdentifier: "KPBO" },
  "outbox_summary": { schemaId: "outbox_summary", fileIdentifier: "KOXS" },
  "action_results": { schemaId: "action_results", fileIdentifier: "KARS" },
  "signed_events": { schemaId: "signed_events", fileIdentifier: "KSEV" },
  "action_stages": { schemaId: "action_stages", fileIdentifier: "KAST" },
  "action_lifecycle": { schemaId: "action_lifecycle", fileIdentifier: "KALC" },
  "relay_diagnostics": { schemaId: "relay_diagnostics", fileIdentifier: "KRDG" },
  "refs.profile": { schemaId: "refs.profile", fileIdentifier: "NRRD" },
  "refs.event": { schemaId: "refs.event", fileIdentifier: "NRRD" },
  "wallet": { schemaId: "nmp.nip47.wallet", fileIdentifier: "NWST" },
  "bunker_handshake": { schemaId: "bunker_handshake", fileIdentifier: "KBHS" },
  "nip46_onboarding": { schemaId: "nip46_onboarding", fileIdentifier: "KN46" },
  "signer_state": { schemaId: "signer_state", fileIdentifier: "KSST" },
  "nmp.follow_list": { schemaId: "nmp.nip02.follow_list", fileIdentifier: "NF02" },
  "nmp.nip29.group_events": { schemaId: "nmp.nip29.group_events", fileIdentifier: "NGEV" },
  "nmp.nip25.reactions": { schemaId: "nmp.nip25.reactions", fileIdentifier: "N25A" },
  "nmp.nip29.discovered_groups": { schemaId: "nmp.nip29.discovered_groups", fileIdentifier: "NDGS" },
  "nmp.nip29.joined_groups": { schemaId: "nmp.nip29.joined_groups", fileIdentifier: "NJGS" },
  "nmp.nip29.group_roster": { schemaId: "nmp.nip29.group_roster", fileIdentifier: "NGRS" },
  "nmp.nip17.dm_inbox": { schemaId: "nmp.nip17.dm_inbox", fileIdentifier: "NDMI" },
  "nmp.nip17.dm_relay_list": { schemaId: "nmp.nip17.dm_relay_list", fileIdentifier: "NDRL" },
  "nmp.nip51.mute_list": { schemaId: "nmp.nip51.mute_list", fileIdentifier: "NMUT" },
  "nmp.nip51.bookmarks": { schemaId: "nmp.nip51.bookmarks", fileIdentifier: "N51L" },
  "nmp.nip23.articles": { schemaId: "nmp.nip23.articles", fileIdentifier: "NL23" },
  "nmp.wot.bootstrap": { schemaId: "nmp.wot.bootstrap", fileIdentifier: "NWBS" },
  "nmp.notifications": { schemaId: "nmp.notifications", fileIdentifier: "NNTF" },
  "refs.event.envelopes": { schemaId: "refs.event.envelopes", fileIdentifier: "NEMB" },
  "nmp.chat.presence": { schemaId: "nmp.chat.presence", fileIdentifier: "NCHP" },
  "nmp.marmot.snapshot": { schemaId: "nmp.marmot.snapshot", fileIdentifier: "NMMS" },
  "nmp.marmot.messages": { schemaId: "nmp.marmot.messages", fileIdentifier: "NMMG" },
};
