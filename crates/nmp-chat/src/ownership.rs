nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.chat",
    crate_name: "nmp-chat",
    summary: "Reusable chat read-state and presence projections; protocol crates own transport.",
    claims: [
        {
            claim_type: "namespace",
            id: "projection.nmp.chat.presence",
            exclusive: true,
            scope: {
                kind: "projection_family",
                value: "nmp.chat.presence.*",
                context: "",
            },
            owns: [
                "per-group chat read-marker projection family",
                "per-group chat unread count projection family",
                "per-group chat typing participant projection family",
            ],
        },
    ],
    notes: [
    ],
}
