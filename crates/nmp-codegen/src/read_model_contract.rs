//! Neutral read-model ownership contracts (#2617).
//!
//! Projection contracts describe exported snapshot/action/schema surfaces.
//! Read-model contracts describe durable internal materializations that must
//! still obey D4: one canonical source, one production writer, read-only
//! cross-crate APIs, and explicit fixture-only seeding.

/// One guarded mutation method for a read model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadModelMutationMethod {
    /// Source token used by the source-surface scanner. This is intentionally a
    /// method-call token (for example `.upsert_view(`) rather than a bare method
    /// name to avoid matching prose or declarations.
    pub token: &'static str,
    /// Production source paths, relative to the workspace root, allowed to call
    /// this mutation method.
    pub allowed_writer_paths: &'static [&'static str],
    /// Test/fixture path fragments allowed to call this mutation method.
    pub fixture_path_fragments: &'static [&'static str],
}

/// One durable internal read model's D4 contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadModelContract {
    /// Stable read-model id.
    pub id: &'static str,
    /// Positive crate ownership claim that owns this materialization.
    pub owner_claim: &'static str,
    /// Crate that owns the materialization.
    pub owner_crate: &'static str,
    /// Canonical event/state source.
    pub canonical_source: &'static str,
    /// Sole production writer.
    pub writer: &'static str,
    /// Concrete cache/read-model type.
    pub materialization: &'static str,
    /// Read-only API exposed across crate boundaries.
    pub read_api: &'static str,
    /// Named readers/consumers.
    pub readers: &'static [&'static str],
    /// Source-version/recompile/projection invalidation path.
    pub invalidation: &'static str,
    /// Explicit test-only direct seed policy, if any.
    pub fixture_policy: &'static str,
    /// Mutation methods guarded by source-surface lint.
    pub mutation_methods: &'static [ReadModelMutationMethod],
}

const FIXTURE_PATHS: &[&str] = &[
    "/tests/",
    "_tests.rs",
    "/tests.rs",
    "test_support",
    "fixture",
];

/// Durable read-model contract manifest.
pub const READ_MODEL_CONTRACT: &[ReadModelContract] = &[
    ReadModelContract {
        id: "nmp.router.mailbox_cache",
        owner_claim: "read_model.nmp.router.mailbox_cache",
        owner_crate: "nmp-router",
        canonical_source: "accepted kind:10002 NIP-65 relay-list events",
        writer: "nmp_router::Kind10002Parser",
        materialization: "nmp_router::InMemoryMailboxCache",
        read_api: "nmp_core::substrate::MailboxCache",
        readers: &[
            "nmp_router::GenericOutboxRouter",
            "nmp_router::Nip65OutboxResolver",
            "nmp_core::kernel::mailboxes::KernelMailboxes",
        ],
        invalidation: "kernel mailbox-change observer bumps planner/recompile state",
        fixture_policy: "test-only fixture helpers may seed parsed kind:10002 facts",
        mutation_methods: &[
            ReadModelMutationMethod {
                token: ".apply_kind10002_update(",
                allowed_writer_paths: &["crates/nmp-router/src/ingest.rs"],
                fixture_path_fragments: FIXTURE_PATHS,
            },
            ReadModelMutationMethod {
                token: ".remove_kind10002_entry(",
                allowed_writer_paths: &["crates/nmp-router/src/ingest.rs"],
                fixture_path_fragments: FIXTURE_PATHS,
            },
        ],
    },
    ReadModelContract {
        id: "nmp.nip01.profile_cache",
        owner_claim: "nostr.kind.0.profile_metadata",
        owner_crate: "nmp-nip01",
        canonical_source: "accepted kind:0 profile-metadata events",
        writer: "nmp_nip01::Kind0Parser",
        materialization: "nmp_nip01::ProfileCache",
        read_api: "nmp_core::substrate::ProfileLookup",
        readers: &[
            "nmp_core profile-card/ref projection paths",
            "nmp_core profile-claim TTL and RAM eviction paths",
            "zap LNURL resolver",
        ],
        invalidation: "kernel profile-change observer bumps profiles_ver/ref-profile rows",
        fixture_policy: "cache-local tests may seed ProfileCache directly after #[cfg(test)]",
        mutation_methods: &[ReadModelMutationMethod {
            token: ".upsert_view(",
            allowed_writer_paths: &["crates/nmp-nip01/src/kind0_parser.rs"],
            fixture_path_fragments: FIXTURE_PATHS,
        }],
    },
    ReadModelContract {
        id: "nmp.nip17.dm_relay_cache",
        owner_claim: "nostr.kind.10050.dm_relay_list",
        owner_crate: "nmp-nip17",
        canonical_source: "accepted kind:10050 NIP-17 DM relay-list events",
        writer: "nmp_nip17::Kind10050Parser",
        materialization: "nmp_nip17::DmRelayCache",
        read_api: "nmp_core::substrate::DmInboxRelayLookup",
        readers: &[
            "nmp_nip17::SendGiftWrappedDmCommand",
            "nmp_core planner #p-tagged inbox routing adapter",
        ],
        invalidation: "kernel DM-relay change observer recompiles #p routed inbox interests",
        fixture_policy: "nmp-nip17 tests may seed DmRelayCache directly",
        mutation_methods: &[ReadModelMutationMethod {
            token: ".upsert(",
            allowed_writer_paths: &["crates/nmp-nip17/src/kind10050_parser.rs"],
            fixture_path_fragments: FIXTURE_PATHS,
        }],
    },
];

/// Look up a read-model contract by stable id.
#[must_use]
pub fn lookup(id: &str) -> Option<&'static ReadModelContract> {
    READ_MODEL_CONTRACT
        .iter()
        .find(|contract| contract.id == id)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn read_model_contract_ids_are_unique() {
        let mut ids = BTreeSet::new();
        for contract in READ_MODEL_CONTRACT {
            assert!(
                ids.insert(contract.id),
                "duplicate read-model contract id {}",
                contract.id
            );
        }
    }

    #[test]
    fn starter_rows_cover_issue_2617_scope() {
        let ids = READ_MODEL_CONTRACT
            .iter()
            .map(|contract| contract.id)
            .collect::<BTreeSet<_>>();
        assert!(ids.contains("nmp.router.mailbox_cache"));
        assert!(ids.contains("nmp.nip01.profile_cache"));
        assert!(ids.contains("nmp.nip17.dm_relay_cache"));
    }
}
