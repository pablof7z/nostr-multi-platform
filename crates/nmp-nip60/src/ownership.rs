nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip60",
    crate_name: "nmp-nip60",
    summary: "NIP-60 Cashu wallet + NIP-61 NutZap + NIP-88 mint discovery event codecs, Cashu proof/DLEQ/P2PK/rollover types, and pure shape validation. NIP mechanics only.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.17375.nip60_wallet",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "17375",
                context: "",
            },
            owns: [
                "NIP-60 wallet config event codec",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.7375.nip60_token",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "7375",
                context: "",
            },
            owns: [
                "NIP-60 unspent-proof token event codec",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.7376.nip60_history",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "7376",
                context: "",
            },
            owns: [
                "NIP-60 spending-history event codec",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.7374.nip60_quote",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "7374",
                context: "",
            },
            owns: [
                "NIP-60 deposit-quote event codec",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.10019.nip61_nutzap_info",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "10019",
                context: "",
            },
            owns: [
                "NIP-61 NutZap info event codec",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.9321.nip61_nutzap",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9321",
                context: "",
            },
            owns: [
                "NIP-61 NutZap event codec and DLEQ verification",
            ],
        },
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
                "NIP-88 Cashu mint announcement decoder",
            ],
        },
        {
            claim_type: "mechanism",
            id: "nip60.cashu_backend_adapter",
            exclusive: true,
            scope: {
                kind: "namespace",
                value: "nmp-nip60",
                context: "",
            },
            owns: [
                "Cashu proof/DLEQ/P2PK/rollover types and the Cashu backend adapter seam surface consumed by nmp-wallet",
            ],
        },
    ],
    notes: [
        {
            claim: "nip60.cashu_backend_adapter",
            text: "Backend selection, the wallet operation journal, the unified WalletBackend seam, and relay-acquisition policy are owned by nmp-wallet (docs/architecture/nip60-nip61-wallet-design.md), not this crate.",
        },
    ],
}
