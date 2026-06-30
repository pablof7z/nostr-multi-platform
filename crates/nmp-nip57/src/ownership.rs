nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip57",
    crate_name: "nmp-nip57",
    summary: "NIP-57 lightning zaps as an NMP protocol crate - ZapReceiptRecord decoder (kind:9735) + ZapRequestBuilder (kind:9734) + ZapAction ActionModule + ZapsView + LNURL-pay fetcher (FetchLnurlInvoiceCommand).",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.9734.zap_request",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9734",
                context: "",
            },
            owns: [
                "zap request builder and LNURL-pay flow input",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.9735.zap_receipt",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9735",
                context: "",
            },
            owns: [
                "zap receipt decoder and zaps view",
            ],
        },
    ],
    notes: [
    ],
}
