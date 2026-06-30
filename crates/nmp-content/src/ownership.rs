nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.content",
    crate_name: "nmp-content",
    summary: "Layer A content-rendering substrate - tokenizer, embed claim registry, recursion guard.",
    claims: [
        {
            claim_type: "mechanism",
            id: "content.rendering",
            exclusive: true,
            scope: {
                kind: "type",
                value: "ContentRenderer",
                context: "",
            },
            owns: [
                "content tokenization, rendering, embed registry, and recursion guard",
            ],
        },
    ],
    notes: [
    ],
}
