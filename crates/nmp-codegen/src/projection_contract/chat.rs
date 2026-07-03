use super::{DeclarationPolicy, PresencePolicy, ProjectionContract, ProjectionTier};

pub const CHAT_PRESENCE: ProjectionContract = ProjectionContract {
    key: "nmp.chat.presence",
    tier: ProjectionTier::HostRegistered,
    producer: "nmp-chat group-scoped presence/read-state session",
    owner_claim: "projection.nmp.chat.presence",
    schema_id: "nmp.chat.presence",
    file_identifier: "NCHP",
    // nmp-chat wire::CHAT_PRESENCE_SCHEMA_VERSION
    version: 1,
    declaration_policy: DeclarationPolicy::RegistrationGated,
    dependency_versions: &[],
    presence_policy: PresencePolicy::None,
};
