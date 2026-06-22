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
const SRC_CLAIMED_EVENT_CONTENT: &str = "claimed_event_content_ver";
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
        schema_id: "active_account",
        file_identifier: "KACT",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_ACTIVE_ACCOUNT],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "claimed_events",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/claimed_events_fb",
        schema_id: "claimed_events",
        file_identifier: "KCEV",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_CLAIMED_EVENT_CONTENT],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "configured_relays",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/configured_relays_fb",
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
        schema_id: "relay_role_options",
        file_identifier: "KRRO",
        version: 1,
        declaration_policy: DeclarationPolicy::SelfDeclaredBuiltin,
        dependency_versions: &[SRC_CONFIGURED_RELAYS],
        presence_policy: PresencePolicy::RevDerived,
    },
    ProjectionContract {
        key: "settings_hub",
        tier: ProjectionTier::KernelBuiltin,
        producer: "nmp-core kernel/typed_projections/settings_hub_fb",
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
        schema_id: "nmp.nip47.wallet",
        file_identifier: "NWST",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "bunker_handshake",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-core actor/typed_projections/bunker_handshake_fb (NIP-46)",
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
        // The op-feed pilot — the only case where the producer key differs from
        // schema_id.
        schema_id: "nmp.nip01.opfeed",
        file_identifier: "NOFS",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.follow_list",
        tier: ProjectionTier::HostRegistered,
        producer: "apps/chirp ffi/register follow_list (NIP-02)",
        // Deliberate key/schema_id split: envelope key vs payload schema id.
        schema_id: "nmp.nip02.follow_list",
        file_identifier: "NF02",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip29.group_chat",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-nip29 register",
        schema_id: "nmp.nip29.group_chat",
        file_identifier: "NGCS",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip29.discovered_groups",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-nip29 register",
        schema_id: "nmp.nip29.discovered_groups",
        file_identifier: "NDGS",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip29.group_defaults",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-nip29 register::wire_group_defaults",
        schema_id: "nmp.nip29.group_defaults",
        file_identifier: "NGDF",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip17.dm_inbox",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-nip17 register",
        schema_id: "nmp.nip17.dm_inbox",
        file_identifier: "NDMI",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip17.dm_relay_list",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-nip17 register",
        schema_id: "nmp.nip17.dm_relay_list",
        file_identifier: "NDRL",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.nip57.zaps",
        tier: ProjectionTier::HostRegistered,
        producer: "apps/chirp ffi/register zaps (NIP-57)",
        schema_id: "nmp.nip57.zaps",
        file_identifier: "NZAP",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "claimed_event_embeds",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-ffi embed_sidecar",
        schema_id: "claimed_event_embeds",
        file_identifier: "NEMB",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.marmot.snapshot",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-marmot ffi (ADR-0039)",
        schema_id: "nmp.marmot.snapshot",
        file_identifier: "NMMS",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
    ProjectionContract {
        key: "nmp.marmot.messages",
        tier: ProjectionTier::HostRegistered,
        producer: "nmp-marmot ffi (ADR-0039)",
        schema_id: "nmp.marmot.messages",
        file_identifier: "NMMG",
        version: 0,
        declaration_policy: DeclarationPolicy::RegistrationGated,
        dependency_versions: &[],
        presence_policy: PresencePolicy::None,
    },
];
