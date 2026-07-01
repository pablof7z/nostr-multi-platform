//! #1723 (epic #1719) — the neutral **projection contract** manifest.
//!
//! This module is the single, platform-neutral source of truth for the facts
//! that previously lived scattered across producer code, the Swift-centred
//! `swift_projections_registry`, the kernel built-in key set, the projection
//! revision dependency table, the typed decoder registries, and the drift
//! gates: each shared projection's **key**, **tier**, **producer**,
//! **schema_id**, **file_identifier**, **schema version**, **declaration policy**,
//! **source-version dependencies**, and **presence policy**.
//!
//! ## What is neutral vs. presentation
//!
//! A [`ProjectionContract`] carries ONLY facts that are independent of any host
//! platform — they describe the projection's identity on the wire and its
//! kernel-side revision/presence semantics. Platform-presentation facts (the
//! Swift property name + Swift value type + the `flatc --swift` reader struct,
//! the Kotlin reader class, etc.) stay in the per-platform decoder registries
//! ([`crate::swift_projections_registry`], [`crate::keyed_projection_row_payload`]),
//! which now DERIVE their neutral columns from this contract and are
//! cross-checked against it by a fail-closed test
//! ([`crate::projection_contract::tests`]).
//!
//! ## What is derived FROM the contract (this slice)
//!
//! - [`kernel_builtin_projection_keys`] — the Tier-2 kernel built-in key set
//!   (was hand-classified in `projection_tier`, now contract-derived).
//! - `nmp-core`'s generated `KERNEL_BUILTIN_PROJECTION_KEYS` const
//!   ([`crate::rust_builtin_keys`]).
//! - `nmp-core`'s generated `BUILTIN_PROJECTION_DEPENDENCIES` revision table
//!   ([`crate::rust_builtin_keys::render_builtin_deps`]) — the source-version
//!   dependency list each Tier-2 projection's derived revision sums over (was
//!   hand-maintained in `kernel/projection_rev/mod.rs`).
//! - The Swift registry's neutral columns (`schema_id` / `file_identifier` /
//!   `key`) are looked up from the contract and asserted identical.
//!
//! Backward-compat is explicitly out of scope (#1723): the migrated kernel
//! built-in family uses ONLY this contract — its old hand-maintained dependency
//! table and tier classification are deleted, not shimmed.
//!
//! OP-feed sessions are deliberately not rows in this projection-key contract:
//! products choose their own projection keys, while `nmp-note-feed` owns the
//! shared `nmp.note_feed.opfeed` / NNFS schema used by those rows.

/// A projection's role in the kernel's projection keyspace.
///
/// Lives here (not in `projection_tier`) because the tier is a neutral fact the
/// contract owns; `projection_tier` re-exports it for the existing import paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionTier {
    /// Tier-2 **kernel-owned built-in**: emitted directly by
    /// `Kernel::snapshot_projections_with_publish_cluster` and gated by the
    /// host-declared consumed-projection set (ADR-0053). These are exactly the
    /// keys `nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS` enumerates.
    KernelBuiltin,
    /// Tier-1 **host/protocol-registered** projection: produced by a
    /// `SnapshotRegistry::register*` closure (wallet, the NIP crates, Marmot,
    /// the op-feed, the embed sidecar). These self-gate by registration and are
    /// never members of the kernel built-in set.
    HostRegistered,
}

/// How a projection enters the consumed-projection / declaration surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationPolicy {
    /// The kernel emits the projection unconditionally (Tier-2 built-in); the
    /// host declares its interest via the consumed-projection set.
    SelfDeclaredBuiltin,
    /// The projection exists only after a host/protocol `register*` call;
    /// registration IS the declaration (Tier-1).
    RegistrationGated,
}

/// How a Tier-2 built-in projection's per-tick presence (Changed / Unchanged /
/// Cleared) is computed by the kernel's revision tracker.
///
/// Tier-1 host-registered projections have no kernel-side revision semantics
/// (the producer closure self-gates), so they carry [`PresencePolicy::None`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresencePolicy {
    /// Presence follows the derived revision: `Changed` iff the summed
    /// `dependency_versions` advanced since the last emit. The default for
    /// kernel built-ins.
    RevDerived,
    /// A per-tick drain: presence is the `Changed → Cleared → Unchanged`
    /// tristate driven by `note_drain_emit` (`action_results`, `signed_events`).
    Drain,
    /// A copy-with-TTL: like [`PresencePolicy::Drain`] but copied (not drained)
    /// each tick with a wall-clock TTL edge (`action_stages`,
    /// `action_lifecycle`).
    CopyWithTtl,
    /// Not a kernel-revision-tracked projection (Tier-1 host registration, or a
    /// keyed row-delta carrier whose presence is per-row).
    None,
}

/// One projection's neutral, platform-independent contract.
///
/// This is the single source for the facts that used to be repeated across the
/// producer `*_fb.rs` constants, the Swift registry, the kernel built-in key
/// set, and the revision dependency table.
pub struct ProjectionContract {
    /// Kernel-emitted projection key (the `TypedProjection.key` the producer
    /// publishes / the `projections` map key). e.g. `"accounts"`,
    /// `"refs.event.envelopes"`, `"refs.profile"`.
    pub key: &'static str,
    /// The projection's role in the keyspace.
    pub tier: ProjectionTier,
    /// A short human-readable note of the producing crate/module — purely for
    /// auditability of the manifest (where this projection originates).
    pub producer: &'static str,
    /// Positive ownership claim that owns this projection key/surface.
    pub owner_claim: &'static str,
    /// `TypedPayload.schema_id` — the buffer's stable schema identity (the
    /// `*_SCHEMA_ID` constant on the producer crate). For most Tier-2 built-ins
    /// `key == schema_id`; follow-list deliberately splits them. App-owned
    /// OP-feed projections are outside this shared projection-key manifest and
    /// use the NNFS schema owned by `nmp-note-feed`.
    pub schema_id: &'static str,
    /// FlatBuffers `file_identifier` (the 4-byte `*_FILE_IDENTIFIER` constant,
    /// e.g. `"KACC"`, `"NWST"`, `"NRRD"`).
    pub file_identifier: &'static str,
    /// `*_SCHEMA_VERSION` — the producer-stamped schema/source version. Every
    /// projection carries its owning crate's real `*_SCHEMA_VERSION` (the keyed
    /// row-delta carriers reuse the row payload's `REFS_SCHEMA_VERSION`). The
    /// fail-closed gate in [`crate::projection_version_gate`] asserts this equals
    /// the producer const so the contract cannot drift from the producers.
    pub version: u32,
    /// How the projection enters the declaration surface.
    pub declaration_policy: DeclarationPolicy,
    /// Source-version counter names this projection's derived revision sums
    /// over (the `kernel/projection_rev` dependency list). Non-empty ONLY for
    /// Tier-2 kernel built-ins; empty for Tier-1 host registrations.
    pub dependency_versions: &'static [&'static str],
    /// How the kernel computes this projection's per-tick presence.
    pub presence_policy: PresencePolicy,
}

// #1723 — the PROJECTION_CONTRACT data table + its SRC_* source-version-counter
// name constants live in the `table` child module (split to keep this file under
// the 500-LOC hard ceiling). Re-exported so `projection_contract::PROJECTION_CONTRACT`
// resolves unchanged.
mod table;
// Marmot host-registered projection entries split out for 500-LOC cap.
mod marmot;
pub use table::PROJECTION_CONTRACT;

/// Look up a projection's contract by its kernel-emitted key. Returns `None`
/// for an unknown key — callers that require the contract use [`contract_for`]
/// which fails closed.
#[must_use]
pub fn lookup(key: &str) -> Option<&'static ProjectionContract> {
    PROJECTION_CONTRACT.iter().find(|c| c.key == key)
}

/// Look up a projection's contract by key, panicking with a descriptive message
/// when the key is absent. Used by the registry-derivation paths where an
/// unknown key is a programming error (a registry entry with no contract), not
/// a recoverable runtime condition. Fail-closed: an absent key never silently
/// yields default metadata.
///
/// # Panics
/// When `key` is not present in [`PROJECTION_CONTRACT`].
#[must_use]
pub fn contract_for(key: &str) -> &'static ProjectionContract {
    lookup(key).unwrap_or_else(|| {
        panic!(
            "no ProjectionContract entry for key {key:?} — every projection key \
             (Swift registry / keyed registry / kernel built-in) MUST have a \
             PROJECTION_CONTRACT entry. Add one to projection_contract.rs."
        )
    })
}

/// The Tier-2 kernel-owned built-in projection key set — every
/// [`PROJECTION_CONTRACT`] entry tagged [`ProjectionTier::KernelBuiltin`].
/// Returned **sorted + deduplicated** for a deterministic generated artifact.
///
/// This is the single source the generated `KERNEL_BUILTIN_PROJECTION_KEYS`
/// const (and therefore the kernel's `consume_all_builtin_projections` set) is
/// derived from. The keyed row-delta carriers (`refs.profile` / `refs.event`)
/// and the out-of-band drains (`signed_events`) are contract entries tagged
/// `KernelBuiltin`, so they enter the set without a separate hand-maintained
/// "built-ins without a shell decoder" list.
#[must_use]
pub fn kernel_builtin_projection_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = PROJECTION_CONTRACT
        .iter()
        .filter(|c| c.tier == ProjectionTier::KernelBuiltin)
        .map(|c| c.key)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// The per-key source-version dependency table for Tier-2 kernel built-ins,
/// derived from the contract. `(key, &[source_counter_name, ...])`, in the same
/// sorted order as [`kernel_builtin_projection_keys`] for a deterministic
/// generated artifact. The single source for `nmp-core`'s
/// `BUILTIN_PROJECTION_DEPENDENCIES`.
#[must_use]
pub fn kernel_builtin_dependencies() -> Vec<(&'static str, &'static [&'static str])> {
    let mut rows: Vec<(&'static str, &'static [&'static str])> = PROJECTION_CONTRACT
        .iter()
        .filter(|c| c.tier == ProjectionTier::KernelBuiltin)
        .map(|c| (c.key, c.dependency_versions))
        .collect();
    rows.sort_unstable_by_key(|(k, _)| *k);
    rows
}

/// The drain projection keys — every [`PROJECTION_CONTRACT`] entry whose
/// [`PresencePolicy`] is [`PresencePolicy::Drain`], sorted for determinism. The
/// single source for `nmp-core`'s `DRAIN_PROJECTION_KEYS`.
#[must_use]
pub fn drain_projection_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = PROJECTION_CONTRACT
        .iter()
        .filter(|c| c.presence_policy == PresencePolicy::Drain)
        .map(|c| c.key)
        .collect();
    keys.sort_unstable();
    keys
}

/// The conditionally-present projection keys — drains ∪ copy-with-TTL: every
/// [`PROJECTION_CONTRACT`] entry whose [`PresencePolicy`] is
/// [`PresencePolicy::Drain`] or [`PresencePolicy::CopyWithTtl`]. The single
/// source for `nmp-core`'s `CONDITIONAL_PRESENCE_KEYS`. Drains are emitted first
/// (matching the prior hand-authored order: drains then copy-with-TTL), each
/// group sorted, for a deterministic generated artifact.
#[must_use]
pub fn rev_conditional_presence_keys() -> Vec<&'static str> {
    let mut drains: Vec<&'static str> = PROJECTION_CONTRACT
        .iter()
        .filter(|c| c.presence_policy == PresencePolicy::Drain)
        .map(|c| c.key)
        .collect();
    drains.sort_unstable();
    let mut copy: Vec<&'static str> = PROJECTION_CONTRACT
        .iter()
        .filter(|c| c.presence_policy == PresencePolicy::CopyWithTtl)
        .map(|c| c.key)
        .collect();
    copy.sort_unstable();
    drains.extend(copy);
    drains
}

#[cfg(test)]
#[path = "projection_contract/tests.rs"]
mod tests;
