nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip89",
    crate_name: "nmp-nip89",
    summary: "Dependency-light NIP-89 client-identity vocabulary: ClientIdentity -> relay User-Agent string + optional NIP-89 `client` tag (31990:<pubkey>:<d> handler). Pure renderers, no kernel seam.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip89.client_identity",
            exclusive: true,
            scope: {
                kind: "tag",
                value: "client",
                context: "nip89.client_identity",
            },
            owns: [
                "NIP-89 client identity rendering and optional client tag vocabulary",
            ],
        },
    ],
    notes: [
    ],
}
