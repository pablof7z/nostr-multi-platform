//! Threading-graph host-registered projection contract entry.
//!
//! Extracted from `table.rs` to keep each file under the 500-LOC hard cap.

use super::{DeclarationPolicy, PresencePolicy, ProjectionContract, ProjectionTier};

/// Registered by the group-threading typed read session (group-scoped at the
/// app layer via the SAME relay-pinned `#h` + kind filter a group-events view
/// opens). nmp-threading owns kind-blind NIP-10 e-tag reply/root grammar; the
/// group scope is composed at the app layer (issue #2719). No iOS Swift
/// consumer yet.
pub const THREADING_GRAPH: ProjectionContract = ProjectionContract {
    key: "nmp.threading.graph",
    tier: ProjectionTier::HostRegistered,
    producer: "NIP-29 group-threading typed read session (#2719)",
    owner_claim: "projection.nmp.threading.graph",
    schema_id: "nmp.threading.graph",
    file_identifier: "NTHR",
    // nmp-threading wire/threading_graph_fb::THREADING_GRAPH_SCHEMA_VERSION
    version: 1,
    declaration_policy: DeclarationPolicy::RegistrationGated,
    dependency_versions: &[],
    presence_policy: PresencePolicy::None,
};
