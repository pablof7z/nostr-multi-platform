nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip47",
    crate_name: "nmp-nip47",
    summary: "NIP-47 Nostr Wallet Connect - Layer-4 NIP crate. Owns the actor-side `WalletRuntime`, the `nmp.wallet.pay_invoice` `ActionModule`, and the three `ProtocolCommand` impls that replace the pre-V-38 `Wallet*` `ActorCommand` variants. After V-38 `nmp-core -> nmp-nwc` is deleted - the dep direction inverts here.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip47.wallet_runtime",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.wallet.pay_invoice",
                context: "",
            },
            owns: [
                "NIP-47 wallet runtime and pay-invoice action semantics",
            ],
        },
    ],
    notes: [
    ],
}
