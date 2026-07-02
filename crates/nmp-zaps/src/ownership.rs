nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.zaps",
    crate_name: "nmp-zaps",
    summary: "App-facing zap-summary read owner: compiles the NIP-57 kind:9735 zap read plan over nmp-nip57's validated receipt decoder and aggregates it into one typed read model for NMP apps.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nmp.zaps.zap_read",
            exclusive: true,
            scope: {
                kind: "type",
                value: "ZapSummaryProjection",
                context: "",
            },
            owns: [
                "zap target resolution",
                "zap read plan construction over nmp-nip57",
                "zap receipt aggregation for one target",
            ],
        },
        {
            claim_type: "projection",
            id: "projection.nmp.zaps.summary",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.zaps.summary",
                context: "",
            },
            owns: [
                "zap-summary read-model projection key family (open_zaps count read)",
            ],
        },
        {
            claim_type: "schema",
            id: "schema.nmp.zaps.summary",
            exclusive: true,
            scope: {
                kind: "schema",
                value: "nmp.zaps.summary",
                context: "",
            },
            owns: [
                "zap-summary FlatBuffers snapshot schema",
            ],
        },
    ],
    notes: [
        {
            claim: "nmp.zaps.zap_read",
            text: "This crate owns the aggregation read model; nmp-nip57 owns kind:9734/9735 decode, bolt11 amount parsing, and zap-request/receipt validation — this crate never re-implements them.",
        },
    ],
}
