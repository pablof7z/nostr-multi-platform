nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip87",
    crate_name: "nmp-nip87",
    summary: "NIP-87 ecash mint discoverability event codecs: kind:38172 Cashu mint announcement (with NUT capability parsing) and kind:38000 mint recommendation. Thin protocol mechanics only — the discovered-mints projection, read interests, and WoT-scoped aggregation live in nmp-wallet.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.38172.mint_announce",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "38172",
                context: "",
            },
            owns: [
                "NIP-87 Cashu mint announcement codec and NUT capability parsing",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.38000.mint_recommend",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "38000",
                context: "",
            },
            owns: [
                "NIP-87 Cashu mint recommendation codec",
            ],
        },
    ],
    notes: [
        {
            claim: "nostr.kind.38172.mint_announce",
            text: "kind:38173 (Fedimint) is explicitly out of scope. Web-of-trust-scoped aggregation of these codecs into a discovered/recommended-mints projection is owned by nmp-wallet (docs/architecture/nip60-nip61-wallet-design.md), not this crate.",
        },
    ],
}
