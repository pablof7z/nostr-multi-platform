nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip92_types",
    crate_name: "nmp-nip92-types",
    summary: "Dependency-light NIP-92 imeta wire/type substrate shared by media-oriented NIP crates.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip92.imeta_types",
            exclusive: true,
            scope: {
                kind: "tag",
                value: "imeta",
                context: "nip92.media_metadata",
            },
            owns: [
                "NIP-92 imeta wire/type substrate",
            ],
        },
    ],
    notes: [
    ],
}
