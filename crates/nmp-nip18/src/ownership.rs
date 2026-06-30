nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip18",
    crate_name: "nmp-nip18",
    summary: "NIP-18 repost decoding and read-surfacing projections for NMP.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.6.repost",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "6",
                context: "",
            },
            owns: [
                "NIP-18 repost decode and action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.16.generic_repost",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "16",
                context: "",
            },
            owns: [
                "NIP-18 generic repost decode and action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.5.delete_for_repost_projection",
            exclusive: false,
            scope: {
                kind: "kind",
                value: "5",
                context: "repost-projection",
            },
            owns: [
                "delete folding for repost projections",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip18.repost",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip18.repost",
                context: "",
            },
            owns: [
                "repost action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip18.quote_repost",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip18.quote_repost",
                context: "",
            },
            owns: [
                "quote repost action namespace",
            ],
        },
    ],
    notes: [
    ],
}
