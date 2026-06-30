//! Marmot action contract entry.
//!
//! Extracted from `table.rs` to keep each file under the 500-LOC hard cap.

use super::{
    ActionContract, ActionDefaultTier, BuilderSupport, PublicReExportPolicy, TypedDispatchPolicy,
};

const PUBLIC_REEXPORT: PublicReExportPolicy = PublicReExportPolicy::OwnerCratePayload;
const TYPED_ONLY: TypedDispatchPolicy = TypedDispatchPolicy::TypedOnly;

pub const MARMOT_ACTION: ActionContract = ActionContract {
    namespace: "nmp.marmot",
    producer: "nmp-marmot explicit installer",
    module_type: "nmp_marmot::install internal action module",
    payload_type: "nmp.marmot flatbuffer payload",
    owner_claim: "action.nmp.marmot",
    schema_id: "nmp.marmot",
    schema_path: "crates/nmp-marmot/schema/marmot_action.fbs",
    root_type: "MarmotActionPayload",
    schema_version: 1,
    file_identifier: "NMMA",
    default_tier: ActionDefaultTier::Marmot,
    builder_support: BuilderSupport::GeneratedMarmotUnion,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};
