nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip47",
    crate_name: "nmp-nip47",
    summary: "NIP-47 Nostr Wallet Connect - Layer-4 NIP crate. Owns NWC protocol mechanics, the actor-side `WalletRuntime`, and the three `ProtocolCommand` impls behind the legacy NWC wallet actions. The product wallet surface is owned by nmp-wallet.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip47.wallet_runtime",
            exclusive: true,
            scope: {
                kind: "namespace",
                value: "nmp-nip47",
                context: "",
            },
            owns: [
                "NIP-47 wallet runtime and pay-invoice action semantics",
            ],
        },
    ],
    notes: [
        {
            claim: "nip47.wallet_runtime",
            text: "The `nmp.wallet.*` action namespaces and `wallet` projection key are exclusively owned by nmp-wallet (see crates/nmp-wallet/src/ownership.rs); nmp-nip47 remains their current NWC runtime implementation but does not itself claim the action namespace.",
        },
    ],
}
