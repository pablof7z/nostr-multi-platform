//! NIP-02 host-registered projection contract entry.
//!
//! Extracted from `table.rs` to keep each file under the 500-LOC hard cap.

use super::{DeclarationPolicy, PresencePolicy, ProjectionContract, ProjectionTier};

pub const FOLLOW_LIST: ProjectionContract = ProjectionContract {
    key: "nmp.follow_list",
    tier: ProjectionTier::HostRegistered,
    producer: "apps/chirp ffi/register follow_list (NIP-02)",
    owner_claim: "projection.nmp.follow_list",
    // Deliberate key/schema_id split: envelope key vs payload schema id.
    schema_id: "nmp.nip02.follow_list",
    file_identifier: "NF02",
    // nmp-nip02 wire/typed_fb::SCHEMA_VERSION
    version: 1,
    declaration_policy: DeclarationPolicy::RegistrationGated,
    dependency_versions: &[],
    presence_policy: PresencePolicy::None,
};
