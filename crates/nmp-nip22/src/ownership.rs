nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip22",
    crate_name: "nmp-nip22",
    summary: "NIP-22 comment (kind:1111) decode, threaded comment projection, and post-comment action for NMP apps.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.1111.comment",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "1111",
                context: "",
            },
            owns: [
                "NIP-22 comment construction, decode, and threaded projection",
            ],
        },
    ],
    notes: [
    ],
}
