//! Marmot host-registered projection contract entries.
//!
//! Extracted from `table.rs` to keep each file under the 500-LOC hard cap.

use super::{DeclarationPolicy, PresencePolicy, ProjectionContract, ProjectionTier};

pub const MARMOT_SNAPSHOT: ProjectionContract = ProjectionContract {
    key: "nmp.marmot.snapshot",
    tier: ProjectionTier::HostRegistered,
    producer: "nmp-marmot explicit installer",
    owner_claim: "projection.nmp.marmot.snapshot",
    schema_id: "nmp.marmot.snapshot",
    file_identifier: "NMMS",
    // nmp-marmot wire/snapshot_fb::SCHEMA_VERSION
    version: 5,
    declaration_policy: DeclarationPolicy::RegistrationGated,
    dependency_versions: &[],
    presence_policy: PresencePolicy::None,
};

pub const MARMOT_MESSAGES: ProjectionContract = ProjectionContract {
    key: "nmp.marmot.messages",
    tier: ProjectionTier::HostRegistered,
    producer: "nmp-marmot explicit installer",
    owner_claim: "projection.nmp.marmot.messages",
    schema_id: "nmp.marmot.messages",
    file_identifier: "NMMG",
    // nmp-marmot wire/messages_fb::SCHEMA_VERSION
    version: 1,
    declaration_policy: DeclarationPolicy::RegistrationGated,
    dependency_versions: &[],
    presence_policy: PresencePolicy::None,
};
