nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip02",
    crate_name: "nmp-nip02",
    summary: "NIP-02 follow-list actions and projections for NMP apps.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.3.contact_list",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "3",
                context: "",
            },
            owns: [
                "contact/follow list actions and projections",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.follow_list",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.follow_list",
                context: "",
            },
            owns: [
                "follow-list projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.follow",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.follow",
                context: "",
            },
            owns: [
                "follow action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.unfollow",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.unfollow",
                context: "",
            },
            owns: [
                "unfollow action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.follow_many",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.follow_many",
                context: "",
            },
            owns: [
                "bulk follow action namespace",
            ],
        },
    ],
    notes: [
    ],
}
