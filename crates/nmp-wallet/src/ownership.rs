nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.wallet",
    crate_name: "nmp-wallet",
    summary: "Wallet composition crate. Owns wallet action namespaces, the bounded wallet projection shape, backend capability policy, operation journal state, the unified WalletBackend seam, and the PaymentPort adapter.",
    claims: [
        {
            claim_type: "namespace",
            id: "projection.wallet",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "wallet",
                context: "",
            },
            owns: [
                "bounded wallet projection key and product shape",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.wallet.select_backend",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.select_backend",
                context: "",
            },
            owns: [
                "wallet backend selection action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.wallet.nwc.connect",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.nwc.connect",
                context: "",
            },
            owns: [
                "canonical NWC connect action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.wallet.nwc.disconnect",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.nwc.disconnect",
                context: "",
            },
            owns: [
                "canonical NWC disconnect action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.wallet.connect",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.connect",
                context: "",
            },
            owns: [
                "legacy wallet connect compatibility alias during NWC migration",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.wallet.disconnect",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.disconnect",
                context: "",
            },
            owns: [
                "legacy wallet disconnect compatibility alias during NWC migration",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.wallet.pay_invoice",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.pay_invoice",
                context: "",
            },
            owns: [
                "unified BOLT-11 wallet payment action namespace",
            ],
        },
        {
            claim_type: "mechanism",
            id: "action.nmp.wallet.cashu",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.cashu.*",
                context: "",
            },
            owns: [
                "Cashu wallet action namespace family",
            ],
        },
        {
            claim_type: "mechanism",
            id: "action.nmp.wallet.nutzap",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.nutzap.*",
                context: "",
            },
            owns: [
                "nutzap wallet action namespace family",
            ],
        },
        {
            claim_type: "mechanism",
            id: "wallet.backend_seam",
            exclusive: true,
            scope: {
                kind: "namespace",
                value: "nmp-wallet",
                context: "",
            },
            owns: [
                "WalletBackend seam, capability flags, backend selection policy, operation journal, and PaymentPort adapter",
            ],
        },
    ],
    notes: [
        {
            claim: "wallet.backend_seam",
            text: "NIP-specific protocol mechanics remain in nmp-nip47, nmp-nip57, and nmp-nip60; nmp-wallet composes them behind the product wallet surface.",
        },
    ],
}
