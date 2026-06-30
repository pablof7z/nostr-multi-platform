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
        {
            claim_type: "namespace",
            id: "projection.nmp.nip17.dm_inbox",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.nip17.dm_inbox",
                context: "",
            },
            owns: [
                "DM inbox projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.nip17.dm_relay_list",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.nip17.dm_relay_list",
                context: "",
            },
            owns: [
                "DM relay-list projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip17.send",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip17.send",
                context: "",
            },
            owns: [
                "NIP-17 send action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip17.publish_relay_list",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip17.publish_relay_list",
                context: "",
            },
            owns: [
                "NIP-17 DM relay-list publish action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip17.hydrate_peer_relay_list",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip17.hydrate_peer_relay_list",
                context: "",
            },
            owns: [
                "NIP-17 peer relay-list hydration action namespace",
            ],
        },
    ],
    notes: [
    ],
}
