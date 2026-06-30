nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.relations",
    crate_name: "nmp-relations",
    summary: "Legacy cross-protocol social-relation classifier for NMP - tallies reactions (NIP-25), reposts (NIP-18), and comments (NIP-22) onto a note. Split out of nmp-nip01 (#1728) so the base note/profile/reply crate owns no cross-protocol aggregation.",
    claims: [
        {
            claim_type: "mechanism",
            id: "relations.social_classifier",
            exclusive: true,
            scope: {
                kind: "type",
                value: "RelationClassifier",
                context: "",
            },
            owns: [
                "cross-protocol relation classification over already-owned artifacts",
            ],
        },
    ],
    notes: [
    ],
}
