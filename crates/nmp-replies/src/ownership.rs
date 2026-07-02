nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.replies",
    crate_name: "nmp-replies",
    summary: "App-facing reply intent owner: selects the protocol reply shape, builds the reply draft through the owning protocol crate, and compiles reply read plans for NMP apps.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nmp.replies.reply_intent",
            exclusive: true,
            scope: {
                kind: "type",
                value: "Reply",
                context: "",
            },
            owns: [
                "reply target resolution",
                "reply artifact selection across NIP-10 and NIP-22",
                "reply read plan construction",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.replies.reply",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.replies.reply",
                context: "",
            },
            owns: [
                "reply action namespace",
            ],
        },
        {
            claim_type: "schema",
            id: "schema.nmp.replies.reply",
            exclusive: true,
            scope: {
                kind: "schema",
                value: "nmp.replies.reply",
                context: "",
            },
            owns: [
                "reply action FlatBuffers payload schema",
            ],
        },
        {
            claim_type: "projection",
            id: "projection.nmp.replies.summary",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.replies.summary",
                context: "",
            },
            owns: [
                "reply-summary read-model projection key family (open_replies count read)",
            ],
        },
        {
            claim_type: "schema",
            id: "schema.nmp.replies.summary",
            exclusive: true,
            scope: {
                kind: "schema",
                value: "nmp.replies.summary",
                context: "",
            },
            owns: [
                "reply-summary FlatBuffers snapshot schema",
            ],
        },
    ],
    notes: [
        {
            claim: "nmp.replies.reply_intent",
            text: "This crate chooses the reply composition path; nmp-nip01 owns kind:1 short text notes and nmp-nip22 owns kind:1111 comments.",
        },
    ],
}
