nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip05",
    crate_name: "nmp-nip05",
    summary: "NIP-05 reverse resolver for NMP (#1804). Resolves a `name@domain` identifier to a Nostr pubkey via the `.well-known/nostr.json` HTTP endpoint, behind the generic `nmp_core::substrate::ProtocolCommand` seam. Shape parsing is pure; the HTTP round-trip is a blocking worker (native feature).",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip05.reverse_resolution",
            exclusive: true,
            scope: {
                kind: "resolver",
                value: "nip05",
                context: "",
            },
            owns: [
                "NIP-05 identifier parsing and HTTP resolution semantics",
            ],
        },
    ],
    notes: [
    ],
}
