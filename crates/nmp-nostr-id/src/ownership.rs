nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nostr_id",
    crate_name: "nmp-nostr-id",
    summary: "Dependency-light Layer-0 Nostr identifier vocabulary: NIP-19 bech32 entity codec wrappers and the NIP-21 nostr: URI surface, delegated to rust-nostr's canonical codec without reimplementing crypto.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nostr_id.nip19_entity_codec",
            exclusive: true,
            scope: {
                kind: "type",
                value: "Nip19Entity",
                context: "",
            },
            owns: [
                "typed NIP-19 entity vocabulary and encode/decode wrappers",
            ],
        },
        {
            claim_type: "mechanism",
            id: "nostr_id.nip21_uri",
            exclusive: true,
            scope: {
                kind: "type",
                value: "NostrUri",
                context: "",
            },
            owns: [
                "NIP-21 nostr: URI parsing and formatting surface",
            ],
        },
    ],
    notes: [
    ],
}
