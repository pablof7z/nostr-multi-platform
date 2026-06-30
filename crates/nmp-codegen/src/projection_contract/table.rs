//! #1723 — the [`PROJECTION_CONTRACT`] data table (and its `SRC_*`
//! source-version-counter name constants), split out of `projection_contract.rs`
//! to keep that file under the 500-LOC hard ceiling (AGENTS.md). The neutral
//! contract TYPES + the derivation/lookup helpers stay in the parent module;
//! this child owns only the manifest data. Re-exported through the parent so
//! `projection_contract::PROJECTION_CONTRACT` resolves unchanged.

use super::{DeclarationPolicy, PresencePolicy, ProjectionContract, ProjectionTier};

// ── Source-version counter names ───────────────────────────────────────────────
//
// These are the canonical names the `kernel/projection_rev` tracker bumps at
// each domain's write chokepoint. Declared here (not in `nmp-core`) so the
// contract owns the dependency lists; `nmp-core`'s generated dependency table
// references the SAME string literals via `render_builtin_deps`.

const SRC_PROFILES: &str = "profiles_ver";
const SRC_ACTIVE_ACCOUNT: &str = "active_account_ver";
const SRC_ACCOUNTS: &str = "accounts_ver";
const SRC_CONFIGURED_RELAYS: &str = "configured_relays_ver";
const SRC_PUBLISH: &str = "publish_ver";
const SRC_PUBLISH_ENGINE: &str = "publish_engine_ver";
const SRC_DIAGNOSTICS_INPUTS: &str = "diagnostics_inputs_ver";
const SRC_SETTLEMENT_ENQUEUE: &str = "settlement_enqueue_ver";
const SRC_SETTLEMENT_DRAIN: &str = "settlement_drain_ver";
const SRC_TTL_EXPIRY: &str = "ttl_expiry_ver";
const SRC_REF_PROFILE_ROWS: &str = "ref_profile_rows_ver";
const SRC_REF_EVENT_ROWS: &str = "ref_event_rows_ver";

/// The neutral projection contract manifest — every projection the system
/// emits, with its platform-independent identity + kernel-side revision/presence
/// semantics. The single source from which the kernel built-in key set, the
/// revision dependency table, and the Swift/keyed registry neutral columns are
/// derived.
///
/// Ordering is not load-bearing for the derived artifacts (they sort), but the
/// list groups Tier-2 kernel built-ins first, then Tier-1 host registrations,
/// then the keyed row-delta carriers, for readability.
pub const PROJECTION_CONTRACT: &[ProjectionContract] = &[
    // ── Tier-2 kernel-owned built-ins ──────────────────────────────────────────
    ProjectionContract {
        key: "profile",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/profile_fb",
        owner_claim: "projection.profile",
        schema_id: "profile",
        file_identifier: "KPRF",
        version: 2,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_PROFILES, SRC_ACTIVE_ACCOUNT],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "accounts",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/accounts_fb",
        owner_claim: "projection.accounts",
        schema_id: "accounts",
        file_identifier: "KACC",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_ACCOUNTS, SRC_PROFILES],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "active_account",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/active_account_fb",
        owner_claim: "projection.active_account",
        schema_id: "active_account",
        file_identifier: "KACT",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_ACTIVE_ACCOUNT],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "configured_relays",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/configured_relays_fb",
        owner_claim: "projection.configured_relays",
        schema_id: "configured_relays",
        file_identifier: "KCRL",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_CONFIGURED_RELAYS],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "relay_role_options",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/relay_role_options_fb",
        owner_claim: "projection.relay_role_options",
        schema_id: "relay_role_options",
        file_identifier: "KRRO",
        version: 2,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_CONFIGURED_RELAYS],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "settings_hub",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/settings_hub_fb",
        owner_claim: "projection.settings_hub",
        schema_id: "settings_hub",
        file_identifier: "KSHB",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_CONFIGURED_RELAYS],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "publish_queue",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/publish_queue_fb",
        owner_claim: "projection.publish_queue",
        schema_id: "publish_queue",
        file_identifier: "KPBQ",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_PUBLISH],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "publish_outbox",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/publish_outbox_fb",
        owner_claim: "projection.publish_outbox",
        schema_id: "publish_outbox",
        file_identifier: "KPBO",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_PUBLISH_ENGINE],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "outbox_summary",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/outbox_summary_fb",
        owner_claim: "projection.outbox_summary",
        schema_id: "outbox_summary",
        file_identifier: "KOXS",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_PUBLISH_ENGINE],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "action_results",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/action_results_fb",
        owner_claim: "projection.action_results",
        schema_id: "action_results",
        file_identifier: "KARS",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_SETTLEMENT_ENQUEUE, SRC_SETTLEMENT_DRAIN],
        presence_policy: PresencePolicy::Drain,
    },
    ProjectionContract {
        key: "signed_events",
        tier: ProjectionTier::KernelBuiltin,
        // The D13 sign-and-return drain. Kernel-emitted but consumed out-of-band
        // (no SnapshotProjections field / shell decoder).
        producer: "nmp-core kernel/typed_projections/signed_events_fb",
        owner_claim: "projection.signed_events",
        schema_id: "signed_events",
        file_identifier: "KSEV",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_SETTLEMENT_ENQUEUE, SRC_SETTLEMENT_DRAIN],
        presence_policy: PresencePolicy::Drain,
    },
    ProjectionContract {
        key: "action_stages",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/action_stages_fb",
        owner_claim: "projection.action_stages",
        schema_id: "action_stages",
        file_identifier: "KAST",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_SETTLEMENT_ENQUEUE, SRC_TTL_EXPIRY],
        presence_policy: PresencePolicy::CopyWithTtl,
    },
    ProjectionContract {
        key: "action_lifecycle",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/action_lifecycle_fb",
        owner_claim: "projection.action_lifecycle",
        schema_id: "action_lifecycle",
        file_identifier: "KALC",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_SETTLEMENT_ENQUEUE, SRC_TTL_EXPIRY],
        presence_policy: PresencePolicy::CopyWithTtl,
    },
    ProjectionContract {
        key: "relay_diagnostics",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/relay_diagnostics_fb",
        owner_claim: "projection.relay_diagnostics",
        schema_id: "relay_diagnostics",
        file_identifier: "KRDG",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_DIAGNOSTICS_INPUTS],
        presence_policy: PresencePolicy::RevDerived,
    },
    // ── Tier-2 keyed row-delta carriers (kernel built-ins, no shell decoder) ────
    // The producer (`kernel/typed_projections/builtins_refs::ref_row_typed_projection`)
    // stamps `schema_id == key` and `schema_version == REFS_SCHEMA_VERSION (1)` on the
    // NRRD carrier envelope — NOT `"nmp.refs.rowdelta"` (which is the `.fbs`
    // namespace.root, not the envelope schema_id). The contract carries the WIRE
    // truth: the prior keyed-registry `schema_id: "nmp.refs.rowdelta"` field was
    // dead, unread metadata and has been deleted (it never matched the producer).
    ProjectionContract {
        key: "refs.profile",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/builtins_refs (NRRD carrier)",
        owner_claim: "projection.refs.profile",
        schema_id: "refs.profile",
        file_identifier: "NRRD",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_REF_PROFILE_ROWS],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "refs.event",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/builtins_refs (NRRD carrier)",
        owner_claim: "projection.refs.event",
        schema_id: "refs.event",
        file_identifier: "NRRD",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_REF_EVENT_ROWS],
        presence_policy: PresencePolicy::RevDerived,
    },
    // ── Tier-1 host/protocol-registered projections ─────────────────────────────
    // These self-gate by registration (registration IS the declaration), carry
    // no kernel-side revision dependencies, and are never kernel built-ins.
    ProjectionContract {
        key: "wallet",
        tier: ProjectionTier::HostRegistered,
        producer: "apps/chirp wallet_runtime (NIP-47)",
        owner_claim: "projection.wallet",
        schema_id: "nmp.nip47.wallet",
        file_identifier: "NWST",
        // nmp-nip47 wire/typed_fb::SCHEMA_VERSION
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "bunker_handshake",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-core actor/typed_projections/bunker_handshake_fb (NIP-46)",
        owner_claim: "projection.bunker_handshake",
        schema_id: "bunker_handshake",
        file_identifier: "KBHS",
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nip46_onboarding",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-core actor/typed_projections/nip46_onboarding_fb (NIP-46)",
        owner_claim: "projection.nip46_onboarding",
        schema_id: "nip46_onboarding",
        file_identifier: "KN46",
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "signer_state",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-core actor/typed_projections/signer_state_fb (ADR-0048)",
        owner_claim: "projection.signer_state",
        schema_id: "signer_state",
        file_identifier: "KSST",
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.feed.home",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-nip01 op-feed pilot",
        owner_claim: "projection.nmp.feed.home",
        // The op-feed pilot — the only case where the producer key differs from
        // schema_id.
        schema_id: "nmp.nip01.opfeed",
        file_identifier: "NOFS",
        // nmp-nip01 op_feed/typed_wire::OP_FEED_SCHEMA_VERSION
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
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
    },
    ProjectionContract {
        key: "nmp.nip29.group_events",
        tier: ProjectionTier::HostRegistered,
        producer: "NIP-29 group-events typed read session (#2187)",
        owner_claim: "projection.nmp.nip29.group_events",
        schema_id: "nmp.nip29.group_events",
        file_identifier: "NGEV",
        // nmp-nip29 wire/group_events_fb::GROUP_EVENTS_SCHEMA_VERSION
        // v2 — added NIP-10 reply/thread edges (reply_to / root) per group event.
        version: 2,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        // Registered by the reaction-aggregate typed read session (group-scoped
        // at the app layer via a relay-pinned `#h` + `kinds:[7]` filter). NIP-25
        // owns kind:7; the fold is group-agnostic. No iOS Swift consumer yet.
        key: "nmp.nip25.reactions",
        tier: ProjectionTier::HostRegistered,
        producer: "NIP-25 reaction-aggregate typed read session",
        owner_claim: "projection.nmp.nip25.reactions",
        schema_id: "nmp.nip25.reactions",
        file_identifier: "N25A",
        // nmp-nip25 wire/reaction_aggregate_fb::REACTION_AGGREGATE_SCHEMA_VERSION
        // v2 (#2504 follow-up): ReactionTargetAggregate gains `mine` (viewer's
        // own kind:7 ids) for reaction toggle-off (retract).
        version: 2,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip29.discovered_groups",
        tier: ProjectionTier::HostRegistered,
        producer: "NIP-29 group-discovery typed read session (#2088)",
        owner_claim: "projection.nmp.nip29.discovered_groups",
        schema_id: "nmp.nip29.discovered_groups",
        file_identifier: "NDGS",
        // nmp-nip29 wire/discovered_groups_fb::DISCOVERED_GROUPS_SCHEMA_VERSION
        version: 2,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip29.group_defaults",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-nip29 register::wire_group_defaults",
        owner_claim: "projection.nmp.nip29.group_defaults",
        schema_id: "nmp.nip29.group_defaults",
        file_identifier: "NGDF",
        // nmp-nip29 wire/group_defaults_fb::GROUP_DEFAULTS_SCHEMA_VERSION
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        // Registered by `NmpApp::open_nip29_joined_groups_session` (NIP-29 native-runtime read session,
        // #2088 — moved off the prior bare-observer `nmp_nip29::wire_joined_groups`
        // so the view hydrates already-cached membership snapshots). A real
        // Tier-1 projection key with no iOS Swift consumer yet.
        key: "nmp.nip29.joined_groups",
        tier: ProjectionTier::HostRegistered,
        producer: "NIP-29 joined-groups native-runtime read session (#2088)",
        owner_claim: "projection.nmp.nip29.joined_groups",
        schema_id: "nmp.nip29.joined_groups",
        file_identifier: "NJGS",
        // nmp-nip29 wire/joined_groups_fb::JOINED_GROUPS_SCHEMA_VERSION
        version: 2,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        // Registered by `NmpApp::open_nip29_group_roster_session` (NIP-29
        // per-group member roster read session). RETAINS the 39001/39002 member
        // pubkeys + 39003 role catalog the count-only joined/discovered views
        // discard. A real Tier-1 projection key with no iOS Swift consumer yet.
        key: "nmp.nip29.group_roster",
        tier: ProjectionTier::HostRegistered,
        producer: "NIP-29 group-roster native-runtime read session",
        owner_claim: "projection.nmp.nip29.group_roster",
        schema_id: "nmp.nip29.group_roster",
        file_identifier: "NGRS",
        // nmp-nip29 wire/group_roster_fb::GROUP_ROSTER_SCHEMA_VERSION
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip17.dm_inbox",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-nip17 register",
        owner_claim: "projection.nmp.nip17.dm_inbox",
        schema_id: "nmp.nip17.dm_inbox",
        file_identifier: "NDMI",
        // nmp-nip17 wire/dm_inbox_fb::DM_INBOX_SCHEMA_VERSION
        version: 2,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip17.dm_relay_list",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-nip17 register",
        owner_claim: "projection.nmp.nip17.dm_relay_list",
        schema_id: "nmp.nip17.dm_relay_list",
        file_identifier: "NDRL",
        // nmp-nip17 wire/dm_relay_list_fb::DM_RELAY_LIST_SCHEMA_VERSION
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        // Registered by `nmp_defaults::runtimes::mute_runtime` under this key.
        // Was missing from the contract — a real Tier-1 projection key, not an
        // internal wire type (the earlier #1723 investigation misclassified it).
        key: "nmp.nip51.mute_list",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-defaults runtimes/mute_runtime (NIP-51)",
        owner_claim: "projection.nmp.nip51.mute_list",
        schema_id: "nmp.nip51.mute_list",
        file_identifier: "NMUT",
        // nmp-nip51 wire/mute_list_fb::MUTE_LIST_SCHEMA_VERSION
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip51.bookmarks",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-defaults runtimes/bookmarks_runtime (NIP-51)",
        owner_claim: "projection.nmp.nip51.bookmarks",
        schema_id: "nmp.nip51.bookmarks",
        file_identifier: "N51L",
        // nmp-nip51 wire/bookmark_list_fb::BOOKMARK_LIST_SCHEMA_VERSION
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "refs.event.envelopes",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-ffi embed_sidecar",
        owner_claim: "projection.refs.event.envelopes",
        schema_id: "refs.event.envelopes",
        file_identifier: "NEMB",
        // nmp-content wire/embed_sidecar_fb::SCHEMA_VERSION
        version: 2,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.marmot.snapshot",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-marmot ffi (ADR-0039)",
        owner_claim: "projection.nmp.marmot.snapshot",
        schema_id: "nmp.marmot.snapshot",
        file_identifier: "NMMS",
        // nmp-marmot wire/snapshot_fb::SCHEMA_VERSION
        version: 5,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.marmot.messages",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-marmot ffi (ADR-0039)",
        owner_claim: "projection.nmp.marmot.messages",
        schema_id: "nmp.marmot.messages",
        file_identifier: "NMMG",
        // nmp-marmot wire/messages_fb::SCHEMA_VERSION
        version: 1,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
];
