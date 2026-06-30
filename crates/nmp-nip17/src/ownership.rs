nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip17",
    crate_name: "nmp-nip17",
    summary: "NIP-17 private direct messages - kind:14 chat-message rumor builder. The gift-wrap (NIP-59) happens on the actor thread; this crate is a pure rumor builder.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.14.chat_message_rumor",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "14",
                context: "",
            },
            owns: [
                "NIP-17 chat-message rumor construction",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.10050.dm_relay_list",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "10050",
                context: "",
            },
            owns: [
                "DM relay-list parsing and interest semantics",
            ],
        },
    ],
    notes: [
    ],
}
