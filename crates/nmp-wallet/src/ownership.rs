nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.wallet",
    crate_name: "nmp-wallet",
    summary: "Wallet composition crate. Owns wallet action namespaces, the bounded wallet projection shape, backend capability policy, operation journal state, and the unified WalletBackend seam. Selects which backend's PaymentPort adapter NIP-57 pays through; the adapter itself is owned by the crate implementing that backend.",
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
            id: "action.nmp.wallet.connect",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.connect",
                context: "",
            },
            owns: [
                "canonical NWC connect action namespace (implemented today by nmp-nip47 as the sole NWC backend)",
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
                "canonical NWC disconnect action namespace (implemented today by nmp-nip47 as the sole NWC backend)",
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
                "WalletBackend seam, capability flags, backend selection policy, operation journal, and PaymentPort backend selection",
            ],
        },
    ],
    notes: [
        {
            claim: "wallet.backend_seam",
            text: "NIP-specific protocol mechanics remain in nmp-nip47, nmp-nip57, and nmp-nip60; nmp-wallet composes them behind the product wallet surface.",
        },
        {
            claim: "action.nmp.wallet.connect",
            text: "There is exactly one connect/disconnect action name today, not a canonical/legacy pair: nmp-nip47 is the only implementation, under these exact strings. Renaming to a backend-qualified nmp.wallet.nwc.connect/disconnect is epic #2864 Phase 2 (NWC consolidation) work, which moves the ActionModule + wire schema registration out of nmp-nip47 (nmp-nip47's lane, not nmp-wallet's).",
        },
    ],
}
