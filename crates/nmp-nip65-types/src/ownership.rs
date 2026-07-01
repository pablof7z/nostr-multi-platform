nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip65_types",
    crate_name: "nmp-nip65-types",
    summary: "Dependency-light NIP-65 relay-list tag decoder shared by nmp-router and test-support fixtures. Routing/cache/action ownership remains in nmp-router.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip65.relay_list_tag_decoder",
            exclusive: true,
            scope: {
                kind: "wire",
                value: "nostr.kind.10002.tags",
                context: "",
            },
            owns: [
                "canonical decoding of NIP-65 r tags into read/write/both relay sets",
            ],
        },
    ],
    notes: [
    ],
}
