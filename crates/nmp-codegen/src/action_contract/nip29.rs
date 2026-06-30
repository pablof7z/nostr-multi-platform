//! NIP-29 action contract entries — group actions (issue #2170).
//!
//! Extracted from `table.rs` to keep each file under the 500-LOC hard cap.
//! Each constant is a named `ActionContract` entry assembled into
//! `ACTION_CONTRACT` in `table.rs` by name.

use super::{
    ActionContract, ActionDefaultTier, BuilderSupport, PublicReExportPolicy, TypedDispatchPolicy,
};

const PUBLIC_REEXPORT: PublicReExportPolicy = PublicReExportPolicy::OwnerCratePayload;
const TYPED_ONLY: TypedDispatchPolicy = TypedDispatchPolicy::TypedOnly;

pub const NIP29_DISCOVER: ActionContract = ActionContract {
    namespace: "nmp.nip29.discover",
    producer: "nmp-nip29 action",
    module_type: "nmp_nip29::DiscoverGroupsAction",
    payload_type: "nmp_nip29::DiscoverGroupsInput",
    owner_claim: "action.nmp.nip29.discover",
    schema_id: "nmp.nip29.discover",
    schema_path: "crates/nmp-nip29/schema/discover_groups_action.fbs",
    root_type: "DiscoverGroupsPayload",
    schema_version: 1,
    file_identifier: "N29D",
    default_tier: ActionDefaultTier::ComponentRegistered,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const NIP29_PUBLISH_GROUP_EVENT: ActionContract = ActionContract {
    namespace: "nmp.nip29.publish_group_event",
    producer: "nmp-nip29 action",
    module_type: "nmp_nip29::PublishGroupEventAction",
    payload_type: "nmp_nip29::PublishGroupEventInput",
    owner_claim: "action.nmp.nip29.publish_group_event",
    schema_id: "nmp.nip29.publish_group_event",
    schema_path: "crates/nmp-nip29/schema/publish_group_event_action.fbs",
    root_type: "PublishGroupEventPayload",
    schema_version: 1,
    file_identifier: "N29G",
    default_tier: ActionDefaultTier::ComponentRegistered,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const NIP29_JOIN: ActionContract = ActionContract {
    namespace: "nmp.nip29.join",
    producer: "nmp-nip29 action",
    module_type: "nmp_nip29::JoinGroupAction",
    payload_type: "nmp_nip29::JoinGroupInput",
    owner_claim: "action.nmp.nip29.join",
    schema_id: "nmp.nip29.join",
    schema_path: "crates/nmp-nip29/schema/join_group_action.fbs",
    root_type: "JoinGroupPayload",
    schema_version: 1,
    file_identifier: "N29J",
    default_tier: ActionDefaultTier::ComponentRegistered,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const NIP29_CREATE_PUBLIC_GROUP: ActionContract = ActionContract {
    namespace: "nmp.nip29.create_public_group",
    producer: "nmp-nip29 action",
    module_type: "nmp_nip29::CreatePublicGroupAction",
    payload_type: "nmp_nip29::CreatePublicGroupInput",
    owner_claim: "action.nmp.nip29.create_public_group",
    schema_id: "nmp.nip29.create_public_group",
    schema_path: "crates/nmp-nip29/schema/create_public_group_action.fbs",
    root_type: "CreatePublicGroupPayload",
    schema_version: 1,
    file_identifier: "N29P",
    default_tier: ActionDefaultTier::ComponentRegistered,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const NIP29_EDIT_METADATA: ActionContract = ActionContract {
    namespace: "nmp.nip29.edit_metadata",
    producer: "nmp-nip29 action",
    module_type: "nmp_nip29::EditMetadataAction",
    payload_type: "nmp_nip29::EditMetadataInput",
    owner_claim: "action.nmp.nip29.edit_metadata",
    schema_id: "nmp.nip29.edit_metadata",
    schema_path: "crates/nmp-nip29/schema/edit_metadata_action.fbs",
    root_type: "EditMetadataPayload",
    schema_version: 1,
    file_identifier: "N29E",
    default_tier: ActionDefaultTier::ComponentRegistered,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};
