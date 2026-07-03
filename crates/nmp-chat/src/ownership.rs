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
                kind: "projection",
                value: "nmp.chat.presence",
                context: "",
            },
            owns: [
                "chat read-marker projection key",
                "chat unread count projection key",
                "chat typing participant projection key",
            ],
        },
    ],
    notes: [
    ],
}
