pub(crate) struct RuleBBaseline {
    pub(crate) path: &'static str,
    pub(crate) max_hits: usize,
    pub(crate) issue: &'static str,
    pub(crate) reason: &'static str,
}

/// Baseline (tracked debt). Each entry points to an open owner issue and caps
/// the current hit count so baselined files cannot grow new rejected surface.
/// Do NOT add new entries.
pub(crate) const RULE_B_BASELINE: &[RuleBBaseline] = &[
    // #2508 — reject global relation summaries and bucket vocabulary.
    RuleBBaseline {
        path: "crates/nmp-nip01/schema/timeline_snapshot.fbs",
        max_hits: 11,
        issue: "#2508",
        reason: "typed relation-count wire table",
    },
    RuleBBaseline {
        path: "crates/nmp-nip01/src/lib.rs",
        max_hits: 2,
        issue: "#2508",
        reason: "public relation vocabulary re-export",
    },
    RuleBBaseline {
        path: "crates/nmp-nip01/src/note_relations.rs",
        max_hits: 57,
        issue: "#2508",
        reason: "NoteRelationCounts / classifier / buckets",
    },
    RuleBBaseline {
        path: "crates/nmp-nip01/src/timeline_projection.rs",
        max_hits: 19,
        issue: "#2508",
        reason: "timeline cards carry relation summary fields",
    },
    RuleBBaseline {
        path: "crates/nmp-nip01/src/typed_wire/decode.rs",
        max_hits: 14,
        issue: "#2508",
        reason: "decode relation summary wire",
    },
    RuleBBaseline {
        path: "crates/nmp-nip01/src/typed_wire/encode.rs",
        max_hits: 12,
        issue: "#2508",
        reason: "encode relation summary wire",
    },
    // #2512 — storage-specific TargetInteractionCounts / kind classifier debt.
    RuleBBaseline {
        path: "crates/nmp-store/src/events.rs",
        max_hits: 3,
        issue: "#2512",
        reason: "EventStore::interaction_counts API",
    },
    RuleBBaseline {
        path: "crates/nmp-store/src/interaction.rs",
        max_hits: 18,
        issue: "#2512",
        reason: "storage hard-coded interaction classifier",
    },
    RuleBBaseline {
        path: "crates/nmp-store/src/lib.rs",
        max_hits: 1,
        issue: "#2512",
        reason: "TargetInteractionCounts re-export",
    },
    RuleBBaseline {
        path: "crates/nmp-store/src/lmdb/interaction_counters.rs",
        max_hits: 12,
        issue: "#2512",
        reason: "LMDB interaction counter read path",
    },
    RuleBBaseline {
        path: "crates/nmp-store/src/lmdb/store_impl.rs",
        max_hits: 2,
        issue: "#2512",
        reason: "LMDB interaction_counts impl",
    },
    RuleBBaseline {
        path: "crates/nmp-store/src/mem/store_impl.rs",
        max_hits: 7,
        issue: "#2512",
        reason: "memory interaction_counts impl",
    },
    RuleBBaseline {
        path: "crates/nmp-store/src/opfs/store_impl.rs",
        max_hits: 4,
        issue: "#2512",
        reason: "OPFS interaction_counts bridge",
    },
    RuleBBaseline {
        path: "crates/nmp-store/src/types/mod.rs",
        max_hits: 1,
        issue: "#2512",
        reason: "TargetInteractionCounts type export",
    },
    RuleBBaseline {
        path: "crates/nmp-store/src/types/outcomes.rs",
        max_hits: 2,
        issue: "#2512",
        reason: "TargetInteractionCounts type",
    },
    RuleBBaseline {
        path: "crates/nmp-sqlite-wasm/src/interaction_counters.rs",
        max_hits: 29,
        issue: "#2512",
        reason: "SQLite wasm interaction counter read path",
    },
    RuleBBaseline {
        path: "crates/nmp-sqlite-wasm/src/lib.rs",
        max_hits: 1,
        issue: "#2512",
        reason: "TargetInteractionCounts re-export",
    },
    RuleBBaseline {
        path: "crates/nmp-sqlite-wasm/src/types.rs",
        max_hits: 2,
        issue: "#2512",
        reason: "TargetInteractionCounts mirror type",
    },
];

pub(crate) fn baseline_for(path: &str) -> Option<&'static RuleBBaseline> {
    RULE_B_BASELINE.iter().find(|entry| entry.path == path)
}
